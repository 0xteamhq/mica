//! `NetworkPlugin` trait — abstract per-driver network setup so a
//! sandbox is reachable at a known address without driver-specific
//! code leaking into mica core.
//!
//! P4.7 — one impl per driver pair:
//!
//! | Plugin | Used by | What it does |
//! |---|---|---|
//! | `Bridge` | runc / Docker | host bridge + ephemeral host port mapping (already done in `DockerBackend`) |
//! | `Cni` | Kata via K8s | the cluster CNI handles routing; mica uses the headless Service DNS |
//! | `TapNat` | Firecracker / Cloud Hypervisor | per-VM TAP device, NAT'd to the mica container |
//! | `Slirp4netns` | gVisor (rootless) | userspace network stack so no host root is needed |
//!
//! The trait is currently a marker for documentation; impls live
//! inside their respective drivers (P4.2, P4.3) and inside
//! `DockerBackend` / `K8sBackend` for the already-shipped paths.

use async_trait::async_trait;

#[async_trait]
pub trait NetworkPlugin: Send + Sync {
    /// Set up networking for a freshly-launched sandbox. Returns the
    /// address the orchestrator should use to reach the WebDriver
    /// port (e.g. `127.0.0.1:32789` for tap-nat with NAT, or
    /// `pod.svc.cluster.local:4444` for CNI).
    async fn setup(&self, sandbox_id: &str) -> anyhow::Result<String>;

    /// Tear down networking when the sandbox stops. Best-effort —
    /// errors should log, never panic.
    async fn teardown(&self, sandbox_id: &str) -> anyhow::Result<()>;
}

/// Marker types for the four impls we'll fill in across P4.2-P4.5.
pub struct Bridge;
pub struct Cni;
pub struct TapNat;
pub struct Slirp4netns;
