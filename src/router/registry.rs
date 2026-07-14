//! Node registry (nodes.json) + live per-node dynamic state.
//!
//! nodes.json is a public config contract, hot-reloaded on SIGHUP
//! like browsers.json:
//!
//! ```json
//! {
//!   "nodes": [
//!     {
//!       "name": "node-a",
//!       "endpoint": "https://mica-a.internal:4444",
//!       "weight": 2,
//!       "region": "us-east-1",
//!       "labels": { "tier": "spot" },
//!       "username": "router",
//!       "password": "s3cret"
//!     }
//!   ]
//! }
//! ```
//!
//! - `name` — required, stable, `[A-Za-z0-9_.-]+`, unique. Embedded
//!   in session ids; RENAMING OR REMOVING a node orphans its live
//!   sessions. Safe removal: set `weight: 0`, wait for its sessions
//!   to finish (aggregated /status), then delete the entry.
//! - `endpoint` — required `http(s)://host:port`, no trailing slash
//!   (normalized on load). TLS via https.
//! - `weight` — default 1. `0` = route-only: existing sessions still
//!   proxy, node is never picked for new ones (the drain lever).
//! - `username`/`password` — optional Basic credentials the router
//!   presents to the node on every forwarded request, health poll,
//!   and WS handshake. The CLIENT's Authorization header never
//!   reaches nodes.
//! - `region`/`labels` — surfaced in aggregated /status for
//!   dashboards; no routing semantics in v1.

use arc_swap::ArcSwap;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use dashmap::DashMap;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub endpoint: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_weight() -> u32 {
    1
}

impl NodeConfig {
    /// `Basic <b64>` header value for this node, when creds are set.
    pub fn auth_header(&self) -> Option<String> {
        let user = self.username.as_deref()?;
        let pass = self.password.as_deref().unwrap_or("");
        Some(format!(
            "Basic {}",
            BASE64_STANDARD.encode(format!("{user}:{pass}"))
        ))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodesConfig {
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
}

impl NodesConfig {
    pub fn load(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
        let mut cfg: NodesConfig =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse {path}: {e}"))?;
        cfg.validate()?;
        for n in &mut cfg.nodes {
            while n.endpoint.ends_with('/') {
                n.endpoint.pop();
            }
        }
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), String> {
        if self.nodes.is_empty() {
            return Err("nodes.json has no nodes".into());
        }
        let mut seen = std::collections::HashSet::new();
        for n in &self.nodes {
            if n.name.is_empty()
                || !n
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
            {
                return Err(format!(
                    "node name {:?} invalid (allowed: [A-Za-z0-9_.-]+)",
                    n.name
                ));
            }
            if !seen.insert(&n.name) {
                return Err(format!("duplicate node name {:?}", n.name));
            }
            if !n.endpoint.starts_with("http://") && !n.endpoint.starts_with("https://") {
                return Err(format!(
                    "node {:?}: endpoint must be http(s)://host:port, got {:?}",
                    n.name, n.endpoint
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Not successfully polled yet (fresh node) — excluded from create.
    Unknown,
    Healthy,
    /// Node reports `draining: true` — proxied but excluded from create.
    Draining,
    /// Poll failures reached the threshold — proxied but excluded.
    Unhealthy,
}

/// Last parsed /status from a node. Field names track the node's
/// StatusResponse; `draining` defaults false so pre-M2 nodes parse.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StatusSnapshot {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub used: usize,
    #[serde(default)]
    pub queued: usize,
    #[serde(default)]
    pub pending: usize,
    #[serde(default)]
    pub draining: bool,
    #[serde(default)]
    pub browsers: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub sessions: Vec<SessionSnapshot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionSnapshot {
    pub id: String,
    #[serde(default)]
    pub browser: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub started: String,
}

impl StatusSnapshot {
    /// Same match semantics as `Config::find`: empty browser matches
    /// anything; exact version key, else prefix, else default (any
    /// listed version) when no version requested.
    pub fn supports(&self, browser: &str, version: Option<&str>) -> bool {
        if browser.is_empty() {
            return true;
        }
        match self.browsers.get(browser) {
            None => false,
            Some(versions) => match version {
                None | Some("") => true,
                Some(v) => versions.iter().any(|k| k == v || k.starts_with(v)),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeDynamic {
    pub state: NodeState,
    pub consecutive_failures: u32,
    pub snapshot: Option<StatusSnapshot>,
    pub last_seen: Option<SystemTime>,
    pub last_error: Option<String>,
}

impl Default for NodeDynamic {
    fn default() -> Self {
        Self {
            state: NodeState::Unknown,
            consecutive_failures: 0,
            snapshot: None,
            last_seen: None,
            last_error: None,
        }
    }
}

/// Config (hot-swapped) + dynamic health state (persists across
/// reloads — health is a property of the running node, not the file).
#[derive(Default)]
pub struct Registry {
    swap: ArcSwap<NodesConfig>,
    dynamic: DashMap<String, NodeDynamic>,
}

impl Registry {
    pub fn new(cfg: NodesConfig) -> Self {
        let r = Self::default();
        r.apply(cfg);
        r
    }

    /// Swap in a new config: added nodes start Unknown, removed nodes'
    /// dynamic state is dropped (their in-flight sessions become
    /// unroutable — see the module doc for the safe-removal runbook).
    pub fn apply(&self, cfg: NodesConfig) {
        let names: std::collections::HashSet<_> =
            cfg.nodes.iter().map(|n| n.name.clone()).collect();
        self.dynamic.retain(|name, _| names.contains(name));
        for n in &cfg.nodes {
            self.dynamic.entry(n.name.clone()).or_default();
        }
        self.swap.store(Arc::new(cfg));
    }

    pub fn config(&self) -> Arc<NodesConfig> {
        self.swap.load_full()
    }

    pub fn node(&self, name: &str) -> Option<NodeConfig> {
        self.config().nodes.iter().find(|n| n.name == name).cloned()
    }

    pub fn dynamic(&self, name: &str) -> Option<NodeDynamic> {
        self.dynamic.get(name).map(|d| d.clone())
    }

    pub fn set_dynamic(&self, name: &str, d: NodeDynamic) {
        // Ignore nodes that were removed from the config while a poll
        // was in flight.
        if self.dynamic.contains_key(name) {
            self.dynamic.insert(name.to_string(), d);
        }
    }

    pub fn healthy_count(&self) -> usize {
        self.dynamic
            .iter()
            .filter(|d| d.value().state == NodeState::Healthy)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> Result<NodesConfig, String> {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), json).unwrap();
        NodesConfig::load(f.path().to_str().unwrap())
    }

    #[test]
    fn loads_and_normalizes() {
        let c = cfg(r#"{"nodes":[{"name":"a","endpoint":"http://h:4444/"}]}"#).unwrap();
        assert_eq!(c.nodes[0].endpoint, "http://h:4444");
        assert_eq!(c.nodes[0].weight, 1);
    }

    #[test]
    fn rejects_bad_configs() {
        assert!(cfg(r#"{"nodes":[]}"#).is_err(), "empty");
        assert!(
            cfg(r#"{"nodes":[{"name":"a b","endpoint":"http://h"}]}"#).is_err(),
            "bad name"
        );
        assert!(
            cfg(r#"{"nodes":[{"name":"a","endpoint":"h:4444"}]}"#).is_err(),
            "bad endpoint"
        );
        assert!(
            cfg(
                r#"{"nodes":[{"name":"a","endpoint":"http://h"},{"name":"a","endpoint":"http://i"}]}"#
            )
            .is_err(),
            "duplicate"
        );
    }

    #[test]
    fn reload_keeps_dynamic_for_surviving_nodes() {
        let r = Registry::new(
            cfg(r#"{"nodes":[{"name":"a","endpoint":"http://h"},{"name":"b","endpoint":"http://i"}]}"#)
                .unwrap(),
        );
        r.set_dynamic(
            "a",
            NodeDynamic {
                state: NodeState::Healthy,
                ..Default::default()
            },
        );
        // Reload: drop b, add c.
        r.apply(
            cfg(r#"{"nodes":[{"name":"a","endpoint":"http://h"},{"name":"c","endpoint":"http://j"}]}"#)
                .unwrap(),
        );
        assert_eq!(r.dynamic("a").unwrap().state, NodeState::Healthy);
        assert!(r.dynamic("b").is_none(), "removed node dropped");
        assert_eq!(r.dynamic("c").unwrap().state, NodeState::Unknown);
    }

    #[test]
    fn capability_matching() {
        let s: StatusSnapshot =
            serde_json::from_str(r#"{"browsers":{"chrome":["124.0","125.0"]}}"#).unwrap();
        assert!(s.supports("chrome", None));
        assert!(s.supports("chrome", Some("124")));
        assert!(s.supports("chrome", Some("125.0")));
        assert!(!s.supports("chrome", Some("99")));
        assert!(!s.supports("firefox", None));
        assert!(s.supports("", None), "empty browser matches any node");
    }
}
