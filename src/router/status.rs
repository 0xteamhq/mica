//! Aggregated /status (the ggr-ui equivalent), router /ping, /readyz,
//! /metrics.
//!
//! Served from the health poller's cache — no per-request fan-out, so
//! the data is at most one poll interval stale (documented trade-off;
//! placement errors from staleness are absorbed by create failover).
//!
//! Wire shape is a strict SUPERSET of the single-node StatusResponse:
//! same top-level keys (counters summed over Healthy+Draining nodes,
//! browsers unioned, sessions concatenated) plus:
//!   - `"router": true` — dashboards branch on this
//!   - per-session `"node"` — which node owns it
//!   - `"nodes": [...]` — per-node config + health detail
//!
//! Session ids are re-encoded with the node prefix so /vnc/{id} and
//! friends work THROUGH the router.

use super::registry::NodeState;
use super::{RouterState, session_id};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct RouterStatusResponse {
    pub total: usize,
    pub used: usize,
    pub queued: usize,
    pub pending: usize,
    pub draining: bool,
    pub browsers: BTreeMap<String, Vec<String>>,
    pub sessions: Vec<RoutedSessionEntry>,
    pub router: bool,
    pub nodes: Vec<NodeStatusEntry>,
}

#[derive(Serialize)]
pub struct RoutedSessionEntry {
    pub id: String,
    pub browser: String,
    pub version: String,
    pub started: String,
    pub node: String,
}

#[derive(Serialize)]
pub struct NodeStatusEntry {
    pub name: String,
    pub endpoint: String,
    pub weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,
    pub state: &'static str,
    pub draining: bool,
    pub total: usize,
    pub used: usize,
    pub queued: usize,
    pub pending: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn state_name(s: NodeState) -> &'static str {
    match s {
        NodeState::Unknown => "unknown",
        NodeState::Healthy => "healthy",
        NodeState::Draining => "draining",
        NodeState::Unhealthy => "unhealthy",
    }
}

pub fn aggregate(state: &RouterState) -> RouterStatusResponse {
    let cfg = state.registry.config();
    let (mut total, mut used, mut queued, mut pending) = (0, 0, 0, 0);
    let mut browsers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sessions = Vec::new();
    let mut nodes = Vec::new();

    for node in &cfg.nodes {
        let d = state.registry.dynamic(&node.name).unwrap_or_default();
        let counted = matches!(d.state, NodeState::Healthy | NodeState::Draining);
        let snap = d.snapshot.as_ref();
        if counted && let Some(s) = snap {
            total += s.total;
            used += s.used;
            queued += s.queued;
            pending += s.pending;
            for (name, versions) in &s.browsers {
                let entry = browsers.entry(name.clone()).or_default();
                for v in versions {
                    if !entry.contains(v) {
                        entry.push(v.clone());
                    }
                }
            }
            for sess in &s.sessions {
                sessions.push(RoutedSessionEntry {
                    id: session_id::encode(&node.name, &sess.id),
                    browser: sess.browser.clone(),
                    version: sess.version.clone(),
                    started: sess.started.clone(),
                    node: node.name.clone(),
                });
            }
        }
        nodes.push(NodeStatusEntry {
            name: node.name.clone(),
            endpoint: node.endpoint.clone(),
            weight: node.weight,
            region: node.region.clone(),
            labels: node.labels.clone(),
            state: state_name(d.state),
            draining: snap.map(|s| s.draining).unwrap_or(false),
            total: snap.map(|s| s.total).unwrap_or(0),
            used: snap.map(|s| s.used).unwrap_or(0),
            queued: snap.map(|s| s.queued).unwrap_or(0),
            pending: snap.map(|s| s.pending).unwrap_or(0),
            last_seen: d
                .last_seen
                .map(|t| humantime::format_rfc3339(t).to_string()),
            error: d.last_error.clone(),
        });
    }
    for versions in browsers.values_mut() {
        versions.sort();
    }

    RouterStatusResponse {
        total,
        used,
        queued,
        pending,
        draining: false,
        browsers,
        sessions,
        router: true,
        nodes,
    }
}

pub async fn status(State(state): State<RouterState>) -> Json<RouterStatusResponse> {
    Json(aggregate(&state))
}

/// Same Pong-ish shape as the node's /ping so fleet monitoring works
/// unchanged against either tier.
pub async fn ping(State(state): State<RouterState>) -> Json<serde_json::Value> {
    let agg = aggregate(&state);
    Json(serde_json::json!({
        "uptime": humantime::format_duration(super::serve::uptime()).to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        "router": true,
        "sessions": agg.sessions.len(),
        "queue": {
            "total": agg.total,
            "used": agg.used,
            "queued": agg.queued,
            "pending": agg.pending,
        },
    }))
}

pub async fn readyz(State(state): State<RouterState>) -> Response {
    if state.registry.healthy_count() == 0 {
        (StatusCode::SERVICE_UNAVAILABLE, "no healthy nodes").into_response()
    } else {
        (StatusCode::OK, "ok").into_response()
    }
}

pub async fn metrics(State(state): State<RouterState>) -> Response {
    let body = match &state.metrics {
        Some(h) => h.render(),
        None => "# metrics not enabled\n".to_string(),
    };
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}
