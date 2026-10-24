//! Port of the credential helpers, backend.php:516-548 and 6324-6459:
//! `fd_get_api_secret`, `fd_save_api_secret`, `fd_clear_api_secret`,
//! `fd_decrypt_credentials`, `fd_fetch_credentials_from_wordpress`,
//! `fd_load_cached_api_credentials`, `fd_save_cached_api_credentials`.
//!
//! WordPress encrypts api_id/api_hash with AES-256-CBC using
//! `hash('sha256', $botToken, true)` as the key; the IV comes back separately.
//! No bot token or session is ever copied between stacks.

use aes::Aes256;
use cbc::Decryptor;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::httpc::http_json;
use crate::log::log;
use crate::storage::{env, storage_path, storage_dir};

type Aes256CbcDec = Decryptor<Aes256>;

pub fn api_secret_path() -> std::path::PathBuf {
    storage_path("api_secret.key")
}

pub fn get_api_secret() -> String {
    if let Some(s) = env("FD_API_SECRET") {
        return s.trim().to_string();
    }
    std::fs::read_to_string(api_secret_path())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn save_api_secret(secret: &str) {
    if secret.is_empty() {
        return;
    }
    let _ = std::fs::write(api_secret_path(), secret);
}

pub fn clear_api_secret() {
    let _ = std::fs::remove_file(api_secret_path());
}

fn creds_path() -> std::path::PathBuf {
    storage_path("api_credentials.json")
}

pub fn load_cached_api_credentials() -> Option<(i32, String)> {
    if let Ok(text) = std::fs::read_to_string(creds_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let api_id = v.get("api_id").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            let api_hash = v
                .get("api_hash")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if api_id > 0 && !api_hash.is_empty() {
                return Some((api_id, api_hash));
            }
        }
    }
    // Environment overrides (debug only, same intent as $overrides in PHP).
    let env_id = env("FD_API_ID").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
    if let (true, Some(hash)) = (env_id > 0, env("FD_API_HASH")) {
        return Some((env_id, hash));
    }
    None
}

pub fn save_cached_api_credentials(api_id: i32, api_hash: &str) {
    let body = serde_json::json!({"api_id": api_id, "api_hash": api_hash});
    let _ = std::fs::write(creds_path(), body.to_string());
}

fn decode_iv(iv: &str) -> Option<[u8; 16]> {
    if iv.len() == 32 && iv.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(b) = hex::decode(iv) {
            if b.len() == 16 {
                let mut out = [0u8; 16];
                out.copy_from_slice(&b);
                return Some(out);
            }
        }
    }
    if let Ok(b) = STANDARD.decode(iv) {
        if b.len() == 16 {
            let mut out = [0u8; 16];
            out.copy_from_slice(&b);
            return Some(out);
        }
    }
    None
}

fn try_decrypt(key: &[u8; 32], iv: &[u8; 16], input: &[u8]) -> Option<(i32, String)> {
    let mut buf = input.to_vec();
    let pt = Aes256CbcDec::new_from_slices(key, iv)
        .ok()?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    let text = String::from_utf8_lossy(pt).to_string();
    let v: Value = serde_json::from_str(&text).ok()?;
    let api_id = v.get("api_id").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
    let api_hash = v.get("api_hash").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if api_id > 0 && !api_hash.is_empty() {
        return Some((api_id, api_hash));
    }
    None
}

/// `fd_decrypt_credentials()`: AES-256-CBC, key = raw `sha256(botToken)`, PKCS#7.
pub fn decrypt_credentials(encrypted: &str, iv: &str, bot_token: &str) -> Option<(i32, String)> {
    let mut hasher = Sha256::new();
    hasher.update(bot_token.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    let ivb = decode_iv(iv)?;

    if let Ok(raw) = STANDARD.decode(encrypted) {
        if let Some(v) = try_decrypt(&key, &ivb, &raw) {
            return Some(v);
        }
    }
    if encrypted.len() % 2 == 0 && encrypted.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(raw) = hex::decode(encrypted) {
            if let Some(v) = try_decrypt(&key, &ivb, &raw) {
                return Some(v);
            }
        }
    }
    None
}

/// `fd_fetch_credentials_from_wordpress()`: POST `/save-bot-token` and decrypt.
pub async fn fetch_credentials_from_wordpress(bot_token: &str) -> Option<(i32, String)> {
    let url = format!("{}/save-bot-token", crate::config::wp_api_base());
    let resp = http_json(
        &url,
        &[("bot_token".to_string(), bot_token.to_string())],
        "POST",
        30,
    )
    .await;
    if !resp.is_object() {
        return None;
    }
    if let Some(secret) = resp.get("api_secret").and_then(|v| v.as_str()) {
        save_api_secret(secret);
    }
    let encrypted = resp
        .get("encrypted_credentials")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let iv = resp
        .get("encryption_iv")
        .or_else(|| resp.get("credentials_iv"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !encrypted.is_empty() && !iv.is_empty() {
        if let Some((api_id, api_hash)) = decrypt_credentials(encrypted, iv, bot_token) {
            save_cached_api_credentials(api_id, &api_hash);
            return Some((api_id, api_hash));
        }
        log("credential decrypt failed", &serde_json::json!({}));
    }
    None
}

/// Path used by `/api/session` diagnostics only.
pub fn storage_root() -> std::path::PathBuf {
    storage_dir()
}
