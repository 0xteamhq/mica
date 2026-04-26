//! Firecracker driver — KVM microVM, scaffolded.
//!
//! Phase-4 P4.2 ship-shape implementation requires:
//! - `firecracker-rust-sdk` or hand-rolled API-socket calls
//! - pre-baked rootfs from the `mica-rootfs` builder (P4.6)
//! - TAP device per VM, NAT'd to mica (see
//!   `super::network::TapNatPlugin`)
//! - snapshot/restore via Firecracker's snapshot endpoints, packaged
//!   as the OCI artifact format defined in `super::snapshot`
//!
//! This commit lands the structural seam (`Driver::Firecracker`,
//! capability probe, `--isolation=firecracker` selection) and the
//! file's documentation so the in-progress spike (P4.0) can drop
//! the implementation in without touching any other module.

use crate::backend::{BackendError, StartParams, StartedSession};

/// Stub `start` — selecting `--isolation=firecracker` must work end-
/// to-end before the spike completes; until then, return a clear
/// `Unsupported`-style error so operators understand exactly why.
pub async fn start(_params: StartParams) -> Result<StartedSession, BackendError> {
    Err(BackendError::Other(
        "firecracker driver: not yet implemented (P4.0 spike pending)".into(),
    ))
}
