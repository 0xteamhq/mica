//! K8s-style health probes.
//!
//! - `GET /healthz` — liveness. Always 200 while the process can
//!   answer HTTP. Used by K8s `livenessProbe` to decide whether to
//!   restart the pod.
//! - `GET /readyz` — readiness. Returns 503 when the queue is
//!   completely full (no slots, all sessions used) — that's the only
//!   condition under which routing more traffic to this replica is
//!   pointless. Used by K8s `readinessProbe` and Ingress controllers
//!   to drain a replica gracefully.
//! - `GET /metrics` — Prometheus exposition. Renders the recorder
//!   snapshot from `AppState.metrics`.

use crate::observability::names::*;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn readyz(State(state): State<AppState>) -> Response {
    // Draining (manual or shutdown-initiated) wins over queue math:
    // routers and Ingress must stop sending new sessions here.
    if state.draining.load(std::sync::atomic::Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    // Queue full = used == capacity AND no pending. We accept new
    // requests as long as the queue can buffer them, so "ready" is
    // really "can we make forward progress?" — only fail when the
    // pool is fully booked.
    let cap = state.queue.capacity();
    let used = state.queue.used();
    let pending = state.queue.pending();
    if cap > 0 && used >= cap && pending == 0 {
        (StatusCode::SERVICE_UNAVAILABLE, "queue full").into_response()
    } else {
        (StatusCode::OK, "ok").into_response()
    }
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    // Refresh the gauges every render. metrics-rs gauges are sticky;
    // setting them on each scrape gives Prometheus the latest value
    // without us having to instrument every change site.
    metrics::gauge!(QUEUE_CAPACITY).set(state.queue.capacity() as f64);
    metrics::gauge!(QUEUE_USED).set(state.queue.used() as f64);
    metrics::gauge!(QUEUE_QUEUED).set(state.queue.queued() as f64);
    metrics::gauge!(QUEUE_PENDING).set(state.queue.pending() as f64);
    metrics::gauge!(SESSIONS_ACTIVE).set(state.sessions.len() as f64);

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
