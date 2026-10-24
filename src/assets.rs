//! The dashboard, compiled into the binary.
//!
//! [`include_dir!`] bakes `public/` (`index.html`, `app.bundle.js`,
//! `css/bundle.css`, `styles.css`, `res/i18n/*`, `assets/**`) into the executable
//! at build time. That is what makes the server a **single `pencarimovie-server`
//! executable**: nothing has to sit beside it, and there is no separate runtime
//! anywhere in the deployment.
//!
//! Two consequences worth knowing:
//!
//! - A frontend change needs `cargo build --release` again. For iterating on the
//!   UI set `PM_ASSETS=disk` and the server reads `public/` from the checkout
//!   instead (documented in README §1).
//! - Only `public/` is embedded. `storage/` (sessions, `auth.json`,
//!   `api_secret.key`) is state and must never be compiled in.

use include_dir::{include_dir, Dir};

/// `<repo>/public`, embedded at compile time.
///
/// The crate lives at the repository root, so `public/` is a sibling directory:
/// `$CARGO_MANIFEST_DIR/public`. It is the dashboard shared by every distribution
/// channel and the path the release workflow stages from, so it must not move —
/// [`crate::build`]-time `build.rs` resolves the SAME path for its fingerprint.
pub static PUBLIC: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/public");

/// Read an embedded asset by its path relative to `public/`
/// (`"css/bundle.css"`, `"index.html"`).
pub fn get(path: &str) -> Option<&'static [u8]> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return PUBLIC.get_file("index.html").map(|f| f.contents());
    }
    PUBLIC.get_file(path).map(|f| f.contents())
}

pub fn is_empty() -> bool {
    PUBLIC.files().next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_dashboard() {
        assert!(!is_empty(), "public/ must be embedded - the binary serves the UI itself");
        let index = get("index.html").expect("index.html");
        assert!(index.len() > 1000);
        assert!(get("/styles.css").expect("styles.css").len() > 100);
        // Paths outside public/ must not resolve.
        assert!(get("../storage/auth.json").is_none());
        assert!(get("../../backend.php").is_none());
    }
}
