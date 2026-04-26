use super::{Backend, BackendError, HostPorts, StartParams, StartedSession, Stopper};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

/// In-memory backend used by every lifecycle test that doesn't need a
/// real Docker daemon. `MockBackend::new(url)` always succeeds and
/// returns `url` as the upstream; `MockBackend::failing(msg)` returns
/// `BackendError::Docker(msg)` so the WdError mapping path is testable.
pub struct MockBackend {
    upstream: String,
    failure: Option<String>,
    pub stop_count: Arc<AtomicUsize>,
}

impl MockBackend {
    pub fn new(upstream: impl Into<String>) -> Self {
        Self {
            upstream: upstream.into(),
            failure: None,
            stop_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn failing(msg: impl Into<String>) -> Self {
        Self {
            upstream: String::new(),
            failure: Some(msg.into()),
            stop_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

struct CountingStopper {
    count: Arc<AtomicUsize>,
}

#[async_trait]
impl Stopper for CountingStopper {
    async fn stop(self: Box<Self>) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Backend for MockBackend {
    async fn start(&self, _params: StartParams) -> Result<StartedSession, BackendError> {
        if let Some(msg) = &self.failure {
            return Err(BackendError::Docker(msg.clone()));
        }
        Ok(StartedSession {
            upstream: self.upstream.clone(),
            container_id: "mock".into(),
            host_ports: HostPorts::default(),
            started_at: SystemTime::now(),
            stopper: Box::new(CountingStopper {
                count: self.stop_count.clone(),
            }),
        })
    }
}
