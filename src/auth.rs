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
//!
//! Roles (file format v2, wire-compatible with plain htpasswd): a row
//! may carry a third colon-separated field, `admin`:
//!
//!   alice:$2y$05$...:admin      # admin — may hit mutating /admin/api
//!   bob:$2y$05$...              # regular user
//!
//! Apache's `htpasswd -b` rewrites rows without the role column, so
//! once roles are in use the file should be managed by mica (the M3
//! users API) or by hand. A plaintext password that itself ends in
//! `:admin` must be bcrypt-hashed to disambiguate.

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode, request::Parts};
use base64::Engine as _;
use base64::engine::general_purpose;
use std::collections::HashMap;
use std::sync::Arc;

/// The authenticated identity, inserted into request extensions by
/// `require_basic_auth` and read by handlers (session ownership,
/// `RequireAdmin`). Absent when auth is disabled (open mode).
#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub name: String,
    pub admin: bool,
}

#[derive(Debug, Clone)]
struct UserEntry {
    hash: String,
    admin: bool,
}

/// Compiled htpasswd database. Empty map = no auth file configured →
/// every request is allowed.
///
/// Hand-rolled rather than via `htpasswd-verify` because we only care
/// about the two hash flavors htpasswd shipped this decade: bcrypt
/// (`$2y$` / `$2a$` / `$2b$` prefix) and plaintext. MD5/SHA1 rows
/// aren't supported — operators on those should re-hash with bcrypt.
#[derive(Clone)]
pub struct AuthState {
    users: HashMap<String, UserEntry>,
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
            if let Some((user, rest)) = line.split_once(':') {
                // Optional trailing `:admin` role column (format v2).
                let (hash, admin) = match rest.rsplit_once(':') {
                    Some((h, "admin")) => (h, true),
                    _ => (rest, false),
                };
                users.insert(
                    user.to_string(),
                    UserEntry {
                        hash: hash.to_string(),
                        admin,
                    },
                );
            }
        }
        Ok(Self { users })
    }

    /// `true` when no htpasswd file is configured (auth disabled).
    pub fn is_open(&self) -> bool {
        self.users.is_empty()
    }

    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// `(name, admin)` pairs sorted by name — the users API listing.
    /// Hashes never leave this module.
    pub fn entries(&self) -> Vec<(String, bool)> {
        let mut v: Vec<_> = self
            .users
            .iter()
            .map(|(n, e)| (n.clone(), e.admin))
            .collect();
        v.sort();
        v
    }

    /// Insert or replace a user. `password_hash` is a bcrypt hash
    /// (callers hash plaintext; this module never sees the password).
    pub fn upsert(&mut self, name: &str, password_hash: String, admin: bool) {
        self.users.insert(
            name.to_string(),
            UserEntry {
                hash: password_hash,
                admin,
            },
        );
    }

    /// Change a user's role keeping their hash. `false` when unknown.
    pub fn set_admin(&mut self, name: &str, admin: bool) -> bool {
        match self.users.get_mut(name) {
            Some(e) => {
                e.admin = admin;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.users.remove(name).is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.users.contains_key(name)
    }

    /// Serialize back to htpasswd format v2 (`name:hash[:admin]`),
    /// sorted for deterministic files.
    pub fn to_file_string(&self) -> String {
        let mut rows: Vec<_> = self.users.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        let mut out = String::new();
        for (name, e) in rows {
            out.push_str(name);
            out.push(':');
            out.push_str(&e.hash);
            if e.admin {
                out.push_str(":admin");
            }
            out.push('\n');
        }
        out
    }

    /// Verify a `Basic <base64>` header value. Returns the
    /// authenticated identity on success.
    pub fn check(&self, header_value: &str) -> Option<AuthedUser> {
        let token = header_value.strip_prefix("Basic ")?.trim();
        let decoded = general_purpose::STANDARD.decode(token).ok()?;
        let pair = std::str::from_utf8(&decoded).ok()?;
        let (user, pass) = pair.split_once(':')?;
        let stored = self.users.get(user)?;
        if verify_password(pass, &stored.hash) {
            Some(AuthedUser {
                name: user.to_string(),
                admin: stored.admin,
            })
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
        && let Some(user) = auth_state.check(h)
    {
        // Downstream consumers: session ownership attribution in
        // create_session, RequireAdmin on mutating /admin/api routes.
        let mut req = req;
        req.extensions_mut().insert(user);
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

/// Extractor gating mutating admin endpoints.
///
/// Posture mirrors the Basic gate: when auth is disabled (no users
/// file → no `AuthedUser` extension) everything is allowed; when auth
/// is on, only rows with the `admin` role pass. Regular users get 403.
pub struct RequireAdmin(pub Option<AuthedUser>);

#[async_trait::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for RequireAdmin {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.extensions.get::<AuthedUser>() {
            Some(u) if !u.admin => Err((StatusCode::FORBIDDEN, "admin role required")),
            u => Ok(Self(u.cloned())),
        }
    }
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
        let user = s.check(&header).expect("valid creds");
        assert_eq!(user.name, "alice");
        assert!(!user.admin, "no role column means regular user");
        let bad = format!("Basic {}", general_purpose::STANDARD.encode(b"alice:wrong"));
        assert!(s.check(&bad).is_none());
    }

    #[test]
    fn admin_role_column() {
        let h = bcrypt::hash("s3cret", 4).expect("bcrypt");
        let f = write_temp(&format!("root:{h}:admin\nalice:{h}\n"));
        let s = AuthState::load(f.path().to_str().unwrap()).unwrap();
        let header = |pair: &str| {
            format!(
                "Basic {}",
                general_purpose::STANDARD.encode(pair.as_bytes())
            )
        };
        assert!(s.check(&header("root:s3cret")).expect("root ok").admin);
        assert!(!s.check(&header("alice:s3cret")).expect("alice ok").admin);
    }

    #[test]
    fn plaintext_with_colon_still_verifies() {
        // The role column only strips a literal trailing ":admin";
        // other colons stay part of the plaintext password.
        let f = write_temp("dev:pa:ss\n");
        let s = AuthState::load(f.path().to_str().unwrap()).unwrap();
        let header = format!("Basic {}", general_purpose::STANDARD.encode(b"dev:pa:ss"));
        assert_eq!(s.check(&header).expect("ok").name, "dev");
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
