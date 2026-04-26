//! Cloud Hypervisor driver — KVM microVM, vendor-neutral, scaffolded.
//!
//! Mirrors the Firecracker driver: same OCI rootfs, same snapshot
//! envelope, same network plugin (`TapNatPlugin`). Implementation
//! pending until P4.0 spike validates the trait shape on a KVM host.

use crate::backend::{BackendError, StartParams, StartedSession};

pub async fn start(_params: StartParams) -> Result<StartedSession, BackendError> {
    Err(BackendError::Other(
        "cloud_hypervisor driver: not yet implemented (P4.0 spike pending)".into(),
    ))
}
