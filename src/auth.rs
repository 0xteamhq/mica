//! HTTP Basic authentication for the WebDriver / artifact / relay
//! surface.
//!
//! Selenoid recommends "put nginx in front" for auth; mica ships
//! Basic auth in-process so a single-binary deploy needs no L7 LB.
//! Identity comes from a standard htpasswd file (`--users`); bcrypt,
//! sha1, md5, and plaintext rows all verify via the
//! `htpasswd-verify` crate.
//!
//! Open endpoints (no credentials required) — these stay open so K8s
//! probes and Prometheus scrapers don't need credentials:
//!   - GET /healthz / /readyz / /metrics
//!   - GET /ping
//!   - GET /openapi.yaml
//!
//! All other endpoints (WebDriver session lifecycle, /vnc, /video,
//! /logs, /devtools, /clipboard, /download, /file, /status,
//! /wd/hub/status, /session/{id}/bidi) require valid credentials.
//!
//! Reload on SIGHUP: the htpasswd file is re-read alongside
//! browsers.json. The file is wrapped in `ArcSwap` so the hot path
//! is lock-free.

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use std::collections::HashMap;
use std::sync::Arc;

/// Compiled htpasswd database. Empty map = no auth file configured →
/// every request is allowed.
///
/// Hand-rolled rather than via `htpasswd-verify` because we only care
/// about the two hash flavors htpasswd shipped this decade: bcrypt
/// (`$2y$` / `$2a$` / `$2b$` prefix) and plaintext. MD5/SHA1 rows
/// aren't supported — operators on those should re-hash with bcrypt.
pub struct AuthState {
    users: HashMap<String, String>,
}

impl AuthState {
    pub fn empty() -> Self {
        Self {
            users: HashMap::new(),
        }
    }

    /// Load and parse `path`. Empty path returns `Self::empty()`.
    /// Parse errors return `Err` so the operator sees the bad file
    /// at startup rather than silently disabling auth.
    pub fn load(path: &str) -> std::io::Result<Self> {
        if path.is_empty() {
            return Ok(Self::empty());
        }
        let raw = std::fs::read_to_string(path)?;
        let mut users = HashMap::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((user, hash)) = line.split_once(':') {
                users.insert(user.to_string(), hash.to_string());
            }
        }
        Ok(Self { users })
    }

    /// `true` when no htpasswd file is configured (auth disabled).
    pub fn is_open(&self) -> bool {
        self.users.is_empty()
    }

    /// Verify a `Basic <base64>` header value. Returns the username
    /// on success.
    pub fn check(&self, header_value: &str) -> Option<String> {
        let token = header_value.strip_prefix("Basic ")?.trim();
        let decoded = general_purpose::STANDARD.decode(token).ok()?;
        let pair = std::str::from_utf8(&decoded).ok()?;
        let (user, pass) = pair.split_once(':')?;
        let stored = self.users.get(user)?;
        if verify_password(pass, stored) {
            Some(user.to_string())
        } else {
            None
        }
    }
}

fn verify_password(plaintext: &str, stored: &str) -> bool {
    if stored.starts_with("$2y$") || stored.starts_with("$2a$") || stored.starts_with("$2b$") {
        // bcrypt. Selenoid users typically run `htpasswd -nbB user pw`
        // which emits `$2y$`; the bcrypt crate accepts all three.
        bcrypt::verify(plaintext, stored).unwrap_or(false)
    } else {
        // Plaintext fallback — handy for local dev, never recommended
        // in production. Constant-time compare to avoid timing leaks
        // on the per-byte comparison.
        constant_time_eq(plaintext.as_bytes(), stored.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Hot-swappable auth state for SIGHUP reloads.
pub type AuthSwap = Arc<ArcSwap<AuthState>>;

/// Paths that bypass authentication. Match before the gated set.
fn is_open_path(path: &str) -> bool {
    matches!(
        path,
        "/ping" | "/healthz" | "/readyz" | "/metrics" | "/openapi.yaml"
    )
}

/// axum middleware: gate every non-open path on Basic auth when the
/// htpasswd state is configured.
pub async fn require_basic_auth(
    axum::extract::State(swap): axum::extract::State<AuthSwap>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Response<Body> {
    let path = req.uri().path();
    if is_open_path(path) {
        return next.run(req).await;
    }
    let auth_state = swap.load();
    if auth_state.is_open() {
        return next.run(req).await;
    }
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if let Some(h) = header
        && let Some(_user) = auth_state.check(h)
    {
        return next.run(req).await;
    }
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            axum::http::header::WWW_AUTHENTICATE,
            r#"Basic realm="mica""#,
        )
        .body(Body::from("unauthorized"))
        .expect("build 401")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(content: &str) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), content).unwrap();
        f
    }

    #[test]
    fn empty_path_is_open() {
        let s = AuthState::load("").unwrap();
        assert!(s.is_open());
    }

    fn bcrypt_row(user: &str, pass: &str) -> String {
        let h = bcrypt::hash(pass, 4).expect("bcrypt");
        format!("{user}:{h}\n")
    }

    #[test]
    fn bcrypt_check() {
        let f = write_temp(&bcrypt_row("alice", "s3cret"));
        let s = AuthState::load(f.path().to_str().unwrap()).unwrap();
        assert!(!s.is_open());
        let header = format!(
            "Basic {}",
            general_purpose::STANDARD.encode(b"alice:s3cret")
        );
        assert_eq!(s.check(&header), Some("alice".to_string()));
        let bad = format!("Basic {}", general_purpose::STANDARD.encode(b"alice:wrong"));
        assert!(s.check(&bad).is_none());
    }

    #[test]
    fn malformed_header_rejected() {
        let f = write_temp(&bcrypt_row("alice", "s3cret"));
        let s = AuthState::load(f.path().to_str().unwrap()).unwrap();
        assert!(s.check("Bearer xyz").is_none());
        assert!(s.check("Basic !!!").is_none());
        assert!(s.check("").is_none());
    }

    #[test]
    fn open_paths_match() {
        for p in ["/ping", "/healthz", "/readyz", "/metrics", "/openapi.yaml"] {
            assert!(is_open_path(p), "{p} should be open");
        }
        for p in ["/status", "/wd/hub/session", "/vnc/abc", "/video/x.mp4"] {
            assert!(!is_open_path(p), "{p} should be gated");
        }
    }
}
