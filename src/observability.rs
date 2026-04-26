//! Observability — Prometheus metrics export.
//!
//! Single `PrometheusHandle` owned by `AppState`. Handlers and the
//! cancel hook record into the global `metrics` recorder; the
//! handle's `render()` produces the `/metrics` text response on demand.
//!
//! Metric names use the `mica_*` prefix and follow Prometheus naming
//! conventions (snake_case, units in the suffix). Labels are
//! `low-cardinality`-only — never the session id or per-request data.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Build the Prometheus exporter and install it as the global
/// recorder. Returns a handle the `/metrics` handler renders.
pub fn install() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    builder
        .install_recorder()
        .expect("install prometheus recorder")
}

/// Metric-name constants — keep in one place so `/metrics`
/// consumers and instrumentation sites can't drift.
pub mod names {
    pub const SESSIONS_CREATED_TOTAL: &str = "mica_sessions_created_total";
    pub const SESSIONS_FAILED_TOTAL: &str = "mica_sessions_failed_total";
    pub const SESSIONS_TEARDOWN_TOTAL: &str = "mica_sessions_teardown_total";
    pub const SESSION_CREATE_DURATION_MS: &str = "mica_session_create_duration_ms";
    pub const QUEUE_USED: &str = "mica_queue_used";
    pub const QUEUE_PENDING: &str = "mica_queue_pending";
    pub const QUEUE_QUEUED: &str = "mica_queue_queued";
    pub const QUEUE_CAPACITY: &str = "mica_queue_capacity";
    pub const SESSIONS_ACTIVE: &str = "mica_sessions_active";
}
