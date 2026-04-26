//! Capability probe — what the host can actually run.
//!
//! T4.1 — boot-time probe of `/dev/kvm`, `runsc` binary, containerd's
//! Kata runtime, etc. Result drives `select_driver` so operators can
//! pass `--isolation=auto` and get the highest-isolation driver
//! available.

use super::Driver;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    pub kvm: bool,
    pub runsc: bool,
    pub kata_runtime: bool,
    pub docker_socket: bool,
    pub k8s_in_cluster: bool,
}

impl Capabilities {
    pub fn probe() -> Self {
        Self {
            kvm: Path::new("/dev/kvm").exists(),
            runsc: which_in_path("runsc").is_some(),
            kata_runtime: which_in_path("kata-runtime").is_some()
                || which_in_path("containerd-shim-kata-v2").is_some(),
            docker_socket: Path::new("/var/run/docker.sock").exists(),
            k8s_in_cluster: std::env::var("KUBERNETES_SERVICE_HOST").is_ok(),
        }
    }

    pub fn available(&self) -> HashSet<Driver> {
        let mut s = HashSet::new();
        // runc is always available — it's just "plain containers".
        s.insert(Driver::Runc);
        // gVisor can be selected when runsc is on PATH (local) or
        // when we'll be running under K8s (RuntimeClass).
        if self.runsc || self.k8s_in_cluster {
            s.insert(Driver::Gvisor);
        }
        // Kata needs KVM AND either kata-runtime locally or K8s.
        if self.kvm && (self.kata_runtime || self.k8s_in_cluster) {
            s.insert(Driver::Kata);
        }
        // Firecracker / Cloud Hypervisor talk to the host KVM directly.
        if self.kvm {
            s.insert(Driver::Firecracker);
            s.insert(Driver::CloudHypervisor);
        }
        s
    }
}

fn which_in_path(bin: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(bin))
            .find(|p| p.is_file())
    })
}

/// Resolve a user request (e.g. `Some("auto")` or `Some("kata")`) to
/// a concrete driver. `auto` (or unset) picks the highest-score
/// available driver. An explicit pin that isn't available yields a
/// clear error so operators see exactly why their choice was rejected.
pub fn select_driver(
    requested: Option<&str>,
    caps: &Capabilities,
) -> Result<Driver, crate::backend::BackendError> {
    let avail = caps.available();
    match requested {
        None | Some("") | Some("auto") => avail
            .iter()
            .copied()
            .max_by_key(|d| d.isolation_score())
            .ok_or_else(|| {
                crate::backend::BackendError::Other("no isolation drivers available".into())
            }),
        Some(name) => {
            let d: Driver = name.parse()?;
            if avail.contains(&d) {
                Ok(d)
            } else {
                Err(crate::backend::BackendError::Other(format!(
                    "isolation '{}' not available on this host (kvm={}, runsc={}, kata_runtime={}, k8s={})",
                    d.name(),
                    caps.kvm,
                    caps.runsc,
                    caps.kata_runtime,
                    caps.k8s_in_cluster,
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runc_is_always_available() {
        let caps = Capabilities::default();
        assert!(caps.available().contains(&Driver::Runc));
    }

    #[test]
    fn select_auto_picks_strongest() {
        let caps = Capabilities {
            kvm: true,
            runsc: true,
            kata_runtime: true,
            ..Default::default()
        };
        let d = select_driver(Some("auto"), &caps).unwrap();
        // With KVM + Kata + Firecracker + Cloud Hypervisor + gVisor,
        // the picker chooses one of the score-4 drivers.
        assert!(matches!(
            d,
            Driver::Kata | Driver::Firecracker | Driver::CloudHypervisor
        ));
    }

    #[test]
    fn pinned_unavailable_driver_errors() {
        let caps = Capabilities::default();
        let err = select_driver(Some("firecracker"), &caps);
        assert!(err.is_err());
    }

    #[test]
    fn pinned_unknown_name_errors() {
        let caps = Capabilities::default();
        let err = select_driver(Some("nope"), &caps);
        assert!(err.is_err());
    }
}
