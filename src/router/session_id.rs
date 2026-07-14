//! Routed session-id encoding — the wire contract that makes the
//! router stateless.
//!
//! Format: `base64url_nopad(node_name) + "." + upstream_id`
//!
//! The base64url alphabet (`A-Za-z0-9-_`) excludes `.`, so the FIRST
//! dot unambiguously splits the two halves regardless of what the
//! node name or the upstream id contain. Both halves are URL-path
//! safe. Any router replica can decode any request path with no
//! shared state, GGR-style.
//!
//! Wire impact (documented per convention): in router mode, session
//! ids returned from POST /wd/hub/session are NOT the node's UUIDs —
//! clients must treat ids as opaque strings (as W3C requires). Node
//! names travel weakly obfuscated inside ids; treat them as
//! non-secret labels.
//!
//! Alternative considered and rejected: embedding the full endpoint
//! URL would survive registry removal but bloats ids and freezes
//! endpoint/credential config into issued sessions. GGR also resolves
//! through its quota file; name+registry-lookup matches precedent.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode(node_name: &str, upstream_id: &str) -> String {
    format!("{}.{}", URL_SAFE_NO_PAD.encode(node_name), upstream_id)
}

/// True when `frag` carries no `..` path segment. axum percent-decodes
/// captured segments, so `%2F`/`%5C` arrive as literal `/` / `\`, and
/// the `url` crate folds `\` into `/` for http(s) — split on both and
/// reject any `..` segment. `.` is a harmless no-op and stays allowed.
/// The guard that stops a routed id from escaping its node route (see
/// proxy.rs); shared with tail/artifact validation.
pub fn is_traversal_safe(frag: &str) -> bool {
    !frag.split(['/', '\\']).any(|seg| seg == "..")
}

/// `(node_name, upstream_id)`, or None when the prefix is missing or
/// not valid base64url / UTF-8, or the upstream id smuggles a `..`
/// path-traversal segment.
pub fn decode(routed_id: &str) -> Option<(String, String)> {
    let (prefix, upstream) = routed_id.split_once('.')?;
    if prefix.is_empty() || upstream.is_empty() || !is_traversal_safe(upstream) {
        return None;
    }
    let name_bytes = URL_SAFE_NO_PAD.decode(prefix).ok()?;
    let name = String::from_utf8(name_bytes).ok()?;
    if name.is_empty() {
        return None;
    }
    Some((name, upstream.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let id = encode("node-a", "4af1c8e2-aaaa-bbbb-cccc-121212121212");
        let (name, upstream) = decode(&id).expect("decodes");
        assert_eq!(name, "node-a");
        assert_eq!(upstream, "4af1c8e2-aaaa-bbbb-cccc-121212121212");
    }

    #[test]
    fn upstream_id_may_contain_dots() {
        let id = encode("n", "weird.id.with.dots");
        let (name, upstream) = decode(&id).expect("decodes");
        assert_eq!(name, "n");
        assert_eq!(upstream, "weird.id.with.dots");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("no-dot-here").is_none());
        assert!(decode(".empty-prefix").is_none());
        assert!(decode("prefix.").is_none());
        assert!(decode("!!!.id").is_none(), "not base64url");
        // A plain node UUID (as issued by a node directly) has dashes
        // but no dot — must not decode.
        assert!(decode("4af1c8e2-aaaa-bbbb-cccc-121212121212").is_none());
    }

    #[test]
    fn rejects_traversal_in_upstream() {
        // axum decodes `%2F`/`%5C` to `/` / `\` before we see them.
        let node = URL_SAFE_NO_PAD.encode("node-a");
        assert!(decode(&format!("{node}.uuid/../../admin/api/users/eve")).is_none());
        assert!(decode(&format!("{node}.uuid\\..\\..\\admin")).is_none());
        // Three dots → upstream is the bare `..` traversal segment.
        assert!(decode(&format!("{node}...")).is_none());
        // A lone `.` (no-op) and embedded dots are not traversal.
        assert!(decode(&format!("{node}..")).is_some());
        assert!(decode(&format!("{node}.weird.id.with.dots")).is_some());
        assert!(is_traversal_safe("a/b/c.mp4"));
        assert!(!is_traversal_safe("a/../b"));
    }
}
