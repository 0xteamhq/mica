//! Snapshot store — cloud-provider-agnostic OCI artifact format.
//!
//! P4.8 — every driver that supports memory snapshots (Firecracker,
//! Cloud Hypervisor, Kata 3.x) stores them as OCI artifacts in any
//! registry the operator already uses (ECR, GAR, GHCR, Harbor,
//! self-hosted). No proprietary snapshot service.
//!
//! `SnapshotRef` is the addressable identity (`<registry>/<name>@<digest>`).
//! `SnapshotMeta` is the in-band JSON the artifact carries alongside
//! the binary state — driver, base rootfs, kernel command line, the
//! capability set used during the warm boot, and the digest of every
//! file in the snapshot bundle.

use serde::{Deserialize, Serialize};

/// OCI media type for mica memory snapshots.
pub const MICA_SNAPSHOT_MEDIA_TYPE: &str = "application/vnd.mica.snapshot.v1+json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRef {
    /// Registry + repository, e.g. "ghcr.io/0xteamhq/mica/snapshots/firefox-126".
    pub repository: String,
    /// OCI digest, e.g. "sha256:abc...".
    pub digest: String,
}

impl SnapshotRef {
    pub fn pretty(&self) -> String {
        format!("{}@{}", self.repository, self.digest)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub driver: String,
    pub rootfs: SnapshotRef,
    pub kernel: Option<SnapshotRef>,
    pub kernel_cmdline: Option<String>,
    pub captured_at: String,
    /// Sandboxed Chrome version this snapshot was captured against —
    /// used by the warm-pool key so we don't restore a v124 snapshot
    /// when a v126 session is requested.
    pub browser_version: String,
    /// Free-form labels for ops (cost-center, tenant, etc.).
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Push a local memory snapshot bundle to the configured registry,
    /// returning the resulting OCI digest.
    async fn push(
        &self,
        local_path: &std::path::Path,
        repository: &str,
    ) -> anyhow::Result<SnapshotRef>;

    /// Pull a snapshot bundle to a local path so the chosen driver can
    /// restore from it.
    async fn pull(
        &self,
        reference: &SnapshotRef,
        out: &std::path::Path,
    ) -> anyhow::Result<SnapshotMeta>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ref_round_trips_through_json() {
        let r = SnapshotRef {
            repository: "ghcr.io/0xteamhq/mica/snapshots/firefox".into(),
            digest: "sha256:dead".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: SnapshotRef = serde_json::from_str(&s).unwrap();
        assert_eq!(back.pretty(), r.pretty());
    }
}
