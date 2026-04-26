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
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub queue: Queue,
    pub sessions: SessionMap,
    pub backend: Arc<dyn Backend>,
    pub args: Arc<Args>,
    pub http: reqwest::Client,
    pub events: EventBus,
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
            config: Arc::new(config),
            queue,
            sessions: SessionMap::new(),
            backend,
            args: Arc::new(args),
            http,
            events: EventBus::new(),
        }
    }
}
