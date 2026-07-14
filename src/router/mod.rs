//! Router mode (M4) — a stateless GGR-equivalent in the same binary.
//!
//! `mica --router --nodes nodes.json` fronts N mica nodes:
//!
//!   - POST /wd/hub/session — weighted-random placement over healthy,
//!     capability-matching nodes with failover (create.rs); the
//!     returned session id embeds the node name (session_id.rs).
//!   - Everything session-scoped decodes the id prefix and forwards
//!     to the owning node: HTTP via proxy.rs (streamed), /vnc + /bidi
//!     via proxy_ws.rs (shared ws_bridge).
//!   - /status aggregates the health poller's cached node snapshots
//!     into a superset of the single-node shape (status.rs).
//!
//! No Backend, no Queue, no SessionMap — `RouterState` is the whole
//! dependency surface, and docker/k8s/wasmtime are never initialized.
//! Client auth terminates at the router (`--users`); per-node
//! credentials from nodes.json are presented upstream instead. The
//! WASM plugin chain does not run router-side in v1 — nodes retain
//! artifact/lifecycle/HTTP hooks.

pub mod create;
pub mod health;
pub mod proxy;
pub mod proxy_ws;
pub mod registry;
pub mod serve;
pub mod session_id;
pub mod status;

use crate::cli::Args;
use axum::Router;
use axum::routing::{any, get, post};
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;

#[derive(Clone)]
pub struct RouterState {
    pub registry: Arc<registry::Registry>,
    pub args: Arc<Args>,
    /// No global timeout — create/proxy set per-request budgets
    /// (a WebDriver command may legitimately run for minutes).
    pub http: reqwest::Client,
    pub metrics: Option<PrometheusHandle>,
}

/// Route table mirrors the node's (handlers/mod.rs) so clients and
/// dashboards can't tell which tier they're talking to.
pub fn router(state: RouterState) -> Router {
    Router::new()
        .route("/ping", get(status::ping))
        .route("/status", get(status::status))
        .route("/wd/hub/status", get(status::status))
        .route("/healthz", get(healthz))
        .route("/readyz", get(status::readyz))
        .route("/metrics", get(status::metrics))
        .route("/wd/hub/session", post(create::create_session))
        .route("/wd/hub/session/:session_id", any(proxy::session))
        .route(
            "/wd/hub/session/:session_id/*tail",
            any(proxy::session_tail),
        )
        .route("/vnc/:session_id", get(proxy_ws::vnc))
        .route("/session/:session_id/bidi", get(proxy_ws::bidi))
        .route("/devtools/:session_id", any(proxy::devtools))
        .route("/devtools/:session_id/*tail", any(proxy::devtools_tail))
        .route("/clipboard/:session_id", any(proxy::clipboard))
        .route("/download/:session_id", any(proxy::download))
        .route("/download/:session_id/*tail", any(proxy::download_tail))
        .route("/video/:name", any(proxy::video))
        .route("/logs/:name", any(proxy::logs))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}
