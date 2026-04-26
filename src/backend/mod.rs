//! `Backend` trait — the abstraction seam between mica's session
//! lifecycle and the runtime that actually launches a browser.
//!
//! Phase 1 ships two impls: `MockBackend` (used by every lifecycle test
//! that doesn't need real Docker) and `DockerBackend` (filled in across
//! M7 tasks T20-T27). Later phases plug `K8sBackend` (Phase 3) and
//! `MicroVmBackend` / `Isolation` driver chain (Phase 4) in here with
//! no changes to handlers, queue, or session map.
//!
//! The trait surface is deliberately small: `start(params) ->
//! StartedSession`. Stopping is a per-session capability returned with
//! the `StartedSession`, not a free function on the backend, because
//! some implementations (Firecracker snapshots, Kata templating) need
//! to capture state at start time to make stop work.

pub mod docker;
pub mod k8s;
pub mod mock;

use crate::caps::Caps;
use crate::config::browser::Browser;
use crate::error::WdError;
use async_trait::async_trait;
use std::time::SystemTime;
use thiserror::Error;

#[derive(Debug)]
pub struct StartParams {
    pub request_id: String,
    pub caps: Caps,
    pub browser: Browser,
    pub version: String,
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("docker: {0}")]
    Docker(String),
    #[error("timeout waiting for service to become ready")]
    Timeout,
    #[error("other: {0}")]
    Other(String),
}

impl From<BackendError> for WdError {
    fn from(err: BackendError) -> Self {
        // Every backend failure maps to W3C "session not created" with
        // the underlying cause preserved in `message`. Status code
        // (500) is set by WdError's IntoResponse impl.
        WdError::session_not_created(err.to_string())
    }
}

/// What `Backend::start` returns. The caller owns the `Stopper` and is
/// responsible for invoking it (directly via `stop()` or through the
/// session-map cancel hook).
pub struct StartedSession {
    pub upstream: String,
    pub container_id: String,
    pub host_ports: HostPorts,
    pub started_at: SystemTime,
    pub stopper: Box<dyn Stopper>,
}

impl std::fmt::Debug for StartedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartedSession")
            .field("upstream", &self.upstream)
            .field("container_id", &self.container_id)
            .field("host_ports", &self.host_ports)
            .field("started_at", &self.started_at)
            .field("stopper", &"<dyn Stopper>")
            .finish()
    }
}

impl StartedSession {
    pub async fn stop(self) {
        self.stopper.stop().await;
    }
}

/// Host-side ports the session exposes for ancillary endpoints. All
/// optional — gVisor / containerd backends may not surface every port.
#[derive(Default, Debug, Clone)]
pub struct HostPorts {
    pub vnc: Option<String>,
    pub devtools: Option<String>,
    pub fileserver: Option<String>,
    pub clipboard: Option<String>,
}

#[async_trait]
pub trait Stopper: Send + Sync {
    async fn stop(self: Box<Self>);
}

#[async_trait]
pub trait Backend: Send + Sync {
    async fn start(&self, params: StartParams) -> Result<StartedSession, BackendError>;
}
