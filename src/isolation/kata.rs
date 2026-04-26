//! Kata Containers driver — KVM-VM-as-OCI; the K8s-friendly path to
//! VM-grade isolation.
//!
//! Plugs into K8sBackend by setting `runtimeClassName: kata` on the
//! Pod spec. Snapshot/restore is supported by Kata 3.x via VM
//! templating; the snapshot artifact itself rides the OCI envelope
//! defined in `super::snapshot`.
//!
//! Constraint: the cluster nodes must have nested virt enabled
//! (EKS bare metal, GKE nested-virt nodes, on-prem KVM hosts).
//! mica's `Capabilities::probe()` checks for `/dev/kvm` so Kata is
//! only offered when KVM is available.
