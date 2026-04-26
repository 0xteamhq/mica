//! gVisor driver — `runsc` user-space kernel.
//!
//! No KVM required → works on stock managed K8s (EKS / GKE / AKS /
//! Cloud Run / Fargate). When mica's backend is `K8sBackend`, this
//! driver translates to `runtimeClassName: gvisor` on the Pod spec
//! (see `Driver::k8s_runtime_class`). When the backend is local Docker,
//! gVisor is not configured here — operators already running with the
//! `runsc` runtime via Docker's `--runtime=runsc` flag get the same
//! behavior, but mica doesn't manage that toggle in-process today.
//!
//! Cold-start floor: ~80 ms (runsc) plus Phase-2 warm pool gets
//! sessions to ~400 ms end-to-end without KVM.
