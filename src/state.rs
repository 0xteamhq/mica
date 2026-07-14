//! `AppState` — the dependency container handed to every axum handler.
//!
//! Phase 1 ships exactly one shape; Phase 3 (K8s) and Phase 4
//! (Isolation drivers) swap `backend` and `events` here without
//! touching handlers.

use crate::auth::{AuthState, AuthSwap};
use crate::backend::Backend;
use crate::cli::Args;
use crate::config::Config;
use crate::events::EventBus;
use crate::plugins::PluginHost;
use crate::queue::Queue;
use crate::quota::QuotaTable;
use crate::session::SessionMap;
use arc_swap::ArcSwap;
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    /// Hot-swappable config (T51). Read via `config()`; mutate via
    /// `config_swap.store(Arc::new(new_cfg))` from the SIGHUP handler.
    pub config_swap: Arc<ArcSwap<Config>>,
    pub queue: Queue,
    pub sessions: SessionMap,
    pub backend: Arc<dyn Backend>,
    pub args: Arc<Args>,
    pub http: reqwest::Client,
    pub events: EventBus,
    /// Prometheus handle for `/metrics` rendering. `Option` so unit
    /// tests can build an `AppState` without installing a global
    /// recorder (only one process-wide recorder is ever installed).
    pub metrics: Option<PrometheusHandle>,
    /// WASM plugin host. Cancel hook calls
    /// `plugins.artifact_verdict(&FileCreated)` BEFORE emitting the
    /// event onto `events`, so a plugin returning `Skip` / `S3` /
    /// `CustomUri` short-circuits the built-in `S3Uploader`.
    pub plugins: Option<Arc<PluginHost>>,
    /// Node is draining: set by graceful shutdown and by
    /// POST /admin/api/drain. Surfaced on /readyz, /status and the
    /// admin API; create_session rejects while set.
    pub draining: Arc<AtomicBool>,
    /// Hot-swappable htpasswd auth state. Shared with the
    /// `require_basic_auth` middleware so API-triggered reloads
    /// (POST /admin/api/config/reload) and the M3 users API can store
    /// new state that the gate picks up immediately.
    pub auth: AuthSwap,
    /// Per-user session quotas (M3). Default = disabled (unlimited).
    pub quotas: QuotaTable,
    /// Serializes admin-API writes to operator files (browsers.json,
    /// htpasswd, quotas) so concurrent PUTs can't interleave the
    /// tmp-file + rename dance.
    pub file_write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    pub fn new(config: Config, args: Args, backend: Arc<dyn Backend>) -> Self {
        let queue = Queue::new(args.limit);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .expect("build reqwest client");
        Self {
            config_swap: Arc::new(ArcSwap::from_pointee(config)),
            queue,
            sessions: SessionMap::new(),
            backend,
            args: Arc::new(args),
            http,
            events: EventBus::new(),
            metrics: None,
            plugins: None,
            draining: Arc::new(AtomicBool::new(false)),
            auth: Arc::new(ArcSwap::from_pointee(AuthState::empty())),
            quotas: QuotaTable::default(),
            file_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Install quota limits. `main.rs` calls this after loading
    /// `--quotas`; tests set limits directly on `quotas`.
    pub fn with_quotas(self, quotas: crate::quota::Quotas) -> Self {
        self.quotas.store(quotas);
        self
    }

    /// Share the auth swap that the Basic-auth middleware uses.
    /// `main.rs` calls this so admin-API reloads reach the gate.
    pub fn with_auth(mut self, auth: AuthSwap) -> Self {
        self.auth = auth;
        self
    }

    /// Attach a Prometheus handle. `main.rs` calls this after
    /// `observability::install()`; tests leave it unset.
    pub fn with_metrics(mut self, handle: PrometheusHandle) -> Self {
        self.metrics = Some(handle);
        self
    }

    /// Attach a plugin host. Tests leave it unset.
    pub fn with_plugins(mut self, host: Arc<PluginHost>) -> Self {
        self.plugins = Some(host);
        self
    }

    /// Cheap, lock-free snapshot of the current config.
    pub fn config(&self) -> arc_swap::Guard<Arc<Config>> {
        self.config_swap.load()
    }
}
