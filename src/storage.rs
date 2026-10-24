//! Port of the storage foundation (backend.php:64-358):
//! `fd_is_temp_app_dir`, `fd_storage_has_session`, `fd_get_storage_dir`,
//! `fd_storage_path`, `fd_cache_path`, `fd_prune_cache_files`,
//! `fd_limit_file_size`.
//!
//! The on-disk layout is unchanged: `<appRoot>/storage/...`, and every cache
//! file name matches the PHP (`resolve_cache_<md5>.json`, `stream_cache_*.json`,
//! ...) so both runtimes can share one storage directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static STORAGE_DIR: OnceLock<PathBuf> = OnceLock::new();
static LAST_PRUNE: OnceLock<Mutex<i64>> = OnceLock::new();

pub fn env(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

pub fn env_or(key: &str, default: &str) -> String {
    env(key).unwrap_or_else(|| default.to_string())
}

pub fn app_root() -> PathBuf {
    if let Some(p) = env("PM_APP_ROOT") {
        let pb = PathBuf::from(p);
        return fs::canonicalize(&pb).unwrap_or(pb);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Locate the dashboard assets.
///
/// `public/` is PART OF THE DISTRIBUTION, not an optional extra: the binary
/// serves the whole UI (HTML, CSS, `app.bundle.js`, `res/`, `assets/`) straight
/// from disk, with no PHP involved. Resolution order:
///
/// 1. `$PM_APP_ROOT/public`
/// 2. `<cwd>/public`
/// 3. `<exe dir>/public`, then each ancestor up to 5 levels — this covers a
///    packaged `dist/` (exe beside `public/`) and a `target/release/` build in
///    the repo (exe four levels below the checkout root).
pub fn public_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![app_root().join("public")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mut cur = Some(dir);
            for _ in 0..5 {
                match cur {
                    Some(d) => {
                        candidates.push(d.join("public"));
                        cur = d.parent();
                    }
                    None => break,
                }
            }
        }
    }
    for c in candidates {
        if c.join("index.html").is_file() {
            return Some(c);
        }
    }
    None
}

/// PHP: `str_contains($real, 'frankenphp_') || under sys_get_temp_dir()`.
pub fn is_temp_app_dir(dir: &Path) -> bool {
    let real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let tmp = std::env::temp_dir();
    let tmp = fs::canonicalize(&tmp).unwrap_or(tmp);
    if real.starts_with(&tmp) {
        return true;
    }
    real.to_string_lossy().contains("frankenphp_")
}

/// Does this directory look like an existing install?
///
/// `session.madeline` is the marker a **legacy PHP** install leaves behind; it is
/// checked so upgrading in place finds the same `storage/` instead of creating a
/// second one. Nothing here writes that name any more.
pub fn storage_has_session(dir: &Path) -> bool {
    dir.join("session.madeline").exists()
        || dir.join("bot_id.txt").exists()
        || dir.join("session_meta.json").exists()
        || dir.join("sessions").exists()
}

pub fn storage_dir() -> PathBuf {
    STORAGE_DIR
        .get_or_init(|| {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(Path::to_path_buf));
            choose_storage(&app_root(), exe_dir.as_deref(), env("PM_APP_ROOT").is_some())
        })
        .clone()
}

/// Pick the storage directory.
///
/// An **explicit** `PM_APP_ROOT` is an operator decision — a container volume,
/// an Android/Termux data dir, a PaaS mount — so it is used verbatim: no
/// session-preference search and, critically, no [`is_temp_app_dir`] skip. Both
/// of those used to redirect an explicitly rooted server to `<exe dir>/storage`:
/// a temp-rooted server (`mktemp -d`, `/app` on a read-only image, a phone's
/// `files/` dir under `/data/data/...`) silently wrote beside the binary and
/// ignored the directory it was told to use.
///
/// Without `PM_APP_ROOT` the PHP behaviour is kept: prefer the candidate that
/// already holds a session, and never adopt a FrankenPHP temp extraction dir.
fn choose_storage(root: &Path, exe_dir: Option<&Path>, explicit_root: bool) -> PathBuf {
    if explicit_root {
        let dir = root.join("storage");
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    let mut candidates = vec![root.join("storage")];
    if let Some(parent) = exe_dir {
        candidates.push(parent.join("storage"));
    }
    // Prefer a candidate that already holds a session (PHP behaviour),
    // skipping a FrankenPHP temp extraction directory.
    for c in &candidates {
        if is_temp_app_dir(c) {
            continue;
        }
        if c.is_dir() && storage_has_session(c) {
            let _ = fs::create_dir_all(c);
            return c.clone();
        }
    }
    for c in &candidates {
        if is_temp_app_dir(c) {
            continue;
        }
        let _ = fs::create_dir_all(c);
        return c.clone();
    }
    let first = candidates[0].clone();
    let _ = fs::create_dir_all(&first);
    first
}

/// Accepts `storage/foo` or `foo` and always returns `<storageDir>/foo`.
pub fn storage_path(rel: &str) -> PathBuf {
    let mut r = rel.trim_start_matches(['/', '\\']);
    if let Some(x) = r.strip_prefix("storage/").or_else(|| r.strip_prefix("storage\\")) {
        r = x;
    }
    let full = storage_dir().join(r.trim_start_matches(['/', '\\']));
    if let Some(parent) = full.parent() {
        let _ = fs::create_dir_all(parent);
    }
    full
}

pub fn cache_path(name: &str) -> PathBuf {
    let dir = storage_path("cache");
    let _ = fs::create_dir_all(&dir);
    dir.join(name.trim_start_matches(['/', '\\']))
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn mtime_secs(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// TTL map, copied verbatim from `fd_prune_cache_files()` (backend.php:274).
///
/// These prefixes are **legacy**: the live caches moved into `cache.rs` (memory +
/// one redb file), so this table now only sweeps up JSON files written by an older
/// build or by the PHP runtime sharing the directory. Nothing writes them anymore.
pub const CACHE_TTLS: [(&str, i64); 13] = [
    ("resolve_cache_", 7200),
    ("stream_cache_", 300),
    ("cat_cache_", 600),
    ("meta_cache_", 3600),
    ("up_cat_", 3600),
    ("up_meta_", 3600),
    ("sub_cache_", 1800),
    ("cinemeta_", 86400),
    ("upstream_manifest_", 7200),
    ("post_identity_", 604800),
    ("file_title_", 604800),
    ("tt_title_", 604800),
    ("episode_names_", 604800),
];

/// Periodic cache purge. The PHP keeps its throttle in a persisted stamp
/// (`storage/cache/.prune_stamp`) because PHP statics die with the request;
/// this port keeps the same stamp so a shared storage dir is pruned once.
pub fn prune_cache_files(force: bool) {
    let now = now();
    let last = LAST_PRUNE.get_or_init(|| Mutex::new(0));
    if !force {
        if let Ok(guard) = last.lock() {
            if now - *guard < 300 {
                return;
            }
        }
        let stamp = cache_path(".prune_stamp");
        if let Some(m) = mtime_secs(&stamp) {
            if now - m < 300 {
                if let Ok(mut g) = last.lock() {
                    *g = now;
                }
                return;
            }
        }
        let _ = fs::write(&stamp, "");
    }
    if let Ok(mut g) = last.lock() {
        *g = now;
    }

    let cache_dir = cache_path("");
    let storage = storage_dir();

    let purge = |dir: &Path, prefix: &str, ttl: i64, mov: bool| {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(prefix) || !name.ends_with(".json") {
                continue;
            }
            let full = entry.path();
            let m = match mtime_secs(&full) {
                Some(m) => m,
                None => continue,
            };
            if now - m >= ttl {
                let _ = fs::remove_file(&full);
            } else if mov {
                let dest = cache_path(&name);
                if dest != full {
                    let _ = fs::rename(&full, &dest);
                }
            }
        }
    };

    for (prefix, ttl) in CACHE_TTLS.iter() {
        purge(&storage, prefix, *ttl, true);
        purge(&cache_dir, prefix, *ttl, false);
    }

    // The live expiry: drop dead entries from L1 and from the redb file.
    let swept = crate::cache::sweep();
    if swept > 0 {
        crate::log::log(
            "cache swept",
            &serde_json::json!({"entries": swept}),
        );
    }
}

/// `fd_limit_file_size()`: keep the log under 5 MB by dropping the oldest half.
pub fn limit_file_size(path: &Path, max_bytes: u64) {
    let size = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    if size <= max_bytes {
        return;
    }
    if let Ok(buf) = fs::read(path) {
        let keep = (max_bytes / 2) as usize;
        let start = buf.len().saturating_sub(keep);
        let mut out = Vec::from(&b"[... truncated old log entries ...]\n"[..]);
        out.extend_from_slice(&buf[start..]);
        let _ = fs::write(path, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pm-storage-test-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// `PM_APP_ROOT` is the container volume / Android data dir / PaaS mount.
    /// It must win over the session-preference search AND the temp-dir skip,
    /// otherwise the server writes beside its own binary and ignores the
    /// directory it was told to use (the bug this test pins).
    #[test]
    fn explicit_app_root_beats_the_exe_dir_and_the_temp_skip() {
        let root = scratch("root");
        let exe = scratch("exe");
        // A real session beside the binary: the old code preferred this.
        let exe_storage = exe.join("storage");
        fs::create_dir_all(&exe_storage).unwrap();
        fs::write(exe_storage.join("bot_id.txt"), "123").unwrap();

        let picked = choose_storage(&root, Some(&exe), true);
        assert_eq!(picked, root.join("storage"), "explicit root must win");
        assert!(picked.is_dir(), "the binary must create it itself");
        assert!(!picked.starts_with(&exe), "must not be beside the binary");
        // The root itself is under the system temp dir here, which is exactly
        // the case `is_temp_app_dir` used to reject.
        assert!(is_temp_app_dir(&picked));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&exe);
    }

    /// Without `PM_APP_ROOT`, PHP behaviour is kept: a candidate that already
    /// holds a session wins over the plain `<root>/storage`.
    #[test]
    fn without_an_explicit_root_a_session_candidate_is_preferred() {
        let root = scratch("root2");
        let exe = scratch("exe2");
        let exe_storage = exe.join("storage");
        fs::create_dir_all(&exe_storage).unwrap();
        fs::write(exe_storage.join("session.madeline"), "{}").unwrap();

        // `exe` is a temp dir, so it is skipped; the fallback is `<root>/storage`.
        let skipped = choose_storage(&root, Some(&exe), false);
        assert_eq!(skipped, root.join("storage"));
        assert!(storage_has_session(&exe_storage));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&exe);
    }
}
