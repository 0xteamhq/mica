//! `AppState` — the dependency container handed to every axum handler.
//!
//! Phase 1 ships exactly one shape; Phase 3 (K8s) and Phase 4
//! (Isolation drivers) swap `backend` and `events` here without
//! touching handlers.

use crate::backend::Backend;
use crate::cli::Args;
use crate::config::Config;
use crate::events::EventBus;
use crate::queue::Queue;
use crate::session::SessionMap;
use arc_swap::ArcSwap;
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
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
        }
    }

    /// Attach a Prometheus handle. `main.rs` calls this after
    /// `observability::install()`; tests leave it unset.
    pub fn with_metrics(mut self, handle: PrometheusHandle) -> Self {
        self.metrics = Some(handle);
        self
    }

    /// Cheap, lock-free snapshot of the current config.
    pub fn config(&self) -> arc_swap::Guard<Arc<Config>> {
        self.config_swap.load()
    }
}
