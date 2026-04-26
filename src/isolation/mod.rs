//! Phase 4 — pluggable isolation drivers.
//!
//! The point of this module is **no vendor lock-in**. mica's public
//! surface is the `Isolation` trait below; each driver is a feature-
//! gated impl. Operators pick one with `--isolation=<x>`; mica auto-
//! probes the host at boot and selects the highest-isolation driver
//! it can actually run.
//!
//! Driver status (Phase 4 scope):
//!
//! | Driver | Mechanism | KVM | Status |
//! |---|---|---|---|
//! | `runc` | plain containers via DockerBackend | no | done — default |
//! | `gvisor` | user-space kernel (`runsc`); K8s `runtimeClassName: gvisor` | no | done — wired via K8sBackend |
//! | `kata` | KVM-VM-as-OCI; K8s `runtimeClassName: kata` | yes | done — wired via K8sBackend |
//! | `firecracker` | direct KVM microVM | yes | scaffolded (P4.2) |
//! | `cloud_hypervisor` | direct KVM microVM | yes | scaffolded (P4.3) |
//!
//! Same OCI rootfs feeds every driver — that's the lock-in fence,
//! tracked under the `mica-rootfs` builder (P4.6).

pub mod capability;
pub mod cloud_hypervisor;
pub mod firecracker;
pub mod gvisor;
pub mod kata;
pub mod network;
pub mod runc;
pub mod snapshot;

use crate::backend::BackendError;
use std::str::FromStr;

/// Identity of an isolation driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Driver {
    Runc,
    Gvisor,
    Kata,
    Firecracker,
    CloudHypervisor,
}

impl Driver {
    pub const ALL: &'static [Driver] = &[
        Driver::Runc,
        Driver::Gvisor,
        Driver::Kata,
        Driver::CloudHypervisor,
        Driver::Firecracker,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Driver::Runc => "runc",
            Driver::Gvisor => "gvisor",
            Driver::Kata => "kata",
            Driver::Firecracker => "firecracker",
            Driver::CloudHypervisor => "cloud_hypervisor",
        }
    }

    /// Higher score = stronger isolation. The capability probe picks
    /// the highest-score available driver when `--isolation=auto`.
    pub fn isolation_score(self) -> u8 {
        match self {
            Driver::Runc => 1,
            Driver::Gvisor => 2,
            Driver::Kata => 3,
            Driver::CloudHypervisor => 4,
            Driver::Firecracker => 4,
        }
    }

    /// Maps to a Kubernetes `runtimeClassName` when this driver runs
    /// under `K8sBackend`. `None` means "node default" (runc).
    pub fn k8s_runtime_class(self) -> Option<&'static str> {
        match self {
            Driver::Runc => None,
            Driver::Gvisor => Some("gvisor"),
            Driver::Kata => Some("kata"),
            // Firecracker / Cloud Hypervisor run directly on the host,
            // not through a K8s RuntimeClass.
            Driver::Firecracker | Driver::CloudHypervisor => None,
        }
    }
}

impl FromStr for Driver {
    type Err = BackendError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "runc" => Ok(Driver::Runc),
            "gvisor" | "runsc" => Ok(Driver::Gvisor),
            "kata" | "kata-containers" => Ok(Driver::Kata),
            "firecracker" | "fc" => Ok(Driver::Firecracker),
            "cloud_hypervisor" | "ch" => Ok(Driver::CloudHypervisor),
            other => Err(BackendError::Other(format!("unknown isolation: {other}"))),
        }
    }
}
