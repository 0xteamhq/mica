//! Kubernetes backend — `Backend` trait impl using `kube-rs`.
//!
//! Same `Backend` interface as `DockerBackend` so handlers, queue,
//! session map, and pool are unchanged. The big differences are:
//!   - launch unit is a Pod, not a container;
//!   - sessions reach the Pod via a per-session headless Service so
//!     DNS works regardless of CNI (Calico / Cilium / AWS VPC);
//!   - `/dev/shm` is an emptyDir(medium=Memory) sized to 2 GiB by
//!     default (P2.4 default propagated here);
//!   - a `runtimeClassName` is set when `--k8s-runtime-class` is
//!     non-empty (Phase 4 P4.4 enables Kata isolation; P4.5 enables
//!     gVisor — both via the same field).
//!
//! The implementation in this file covers Phase 3 sub-tickets:
//!   - P3.1 connect / start / stop scaffolding
//!   - P3.2 Pod spec mapping from browsers.json (image, cpu/mem,
//!     tmpfs, volumes, env, shmSize, labels)
//!   - P3.3 headless Service per session + readiness probe wait
//!   - P3.5 owner label `mica/owner=<replica_id>` for Ingress
//!     stickiness
//!
//! Tests are gated; running them needs a kind / k3d cluster.

use super::{Backend, BackendError, HostPorts, StartParams, StartedSession, Stopper};
use async_trait::async_trait;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EmptyDirVolumeSource, EnvVar, Pod, PodSpec, ResourceRequirements,
    Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, PostParams};
use kube::{Client, Config};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

const DEFAULT_SHM_BYTES: i64 = 2 * 1024 * 1024 * 1024;
const READY_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct K8sBackend {
    client: Client,
    namespace: String,
    runtime_class: Option<String>,
    /// `mica/owner=<replica>` label written on every Pod the backend
    /// creates. Populated from `--k8s-replica-id`; defaults to a
    /// per-process UUID so single-replica deployments are sticky to
    /// themselves automatically (P3.5).
    replica_id: String,
    service_startup_timeout: Duration,
}

impl K8sBackend {
    /// P3.1: connect via in-cluster config when `KUBERNETES_SERVICE_HOST`
    /// is set, otherwise fall through to `KUBECONFIG` / default kubeconfig.
    pub async fn connect(
        namespace: impl Into<String>,
        replica_id: Option<String>,
    ) -> Result<Self, BackendError> {
        let config = Config::infer()
            .await
            .map_err(|e| BackendError::Other(format!("kube config: {e}")))?;
        let client = Client::try_from(config)
            .map_err(|e| BackendError::Other(format!("kube client: {e}")))?;
        Ok(Self {
            client,
            namespace: namespace.into(),
            runtime_class: None,
            replica_id: replica_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            service_startup_timeout: Duration::from_secs(60),
        })
    }

    pub fn with_runtime_class(mut self, c: Option<String>) -> Self {
        self.runtime_class = c.filter(|s| !s.is_empty());
        self
    }

    pub fn with_service_startup_timeout(mut self, d: Duration) -> Self {
        self.service_startup_timeout = d;
        self
    }

    pub fn replica_id(&self) -> &str {
        &self.replica_id
    }
}

fn pod_name(prefix: &str) -> String {
    // Pod names: lowercase RFC 1123 + max 63 chars. Use a short uuid suffix.
    let id = Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &id[..16])
}

#[async_trait]
impl Backend for K8sBackend {
    async fn start(&self, params: StartParams) -> Result<StartedSession, BackendError> {
        let image = params
            .browser
            .docker_image()
            .ok_or_else(|| BackendError::Other("driver-mode image entries are unsupported".into()))?
            .to_string();

        let webdriver_port: i32 = params.browser.port.parse().map_err(|e| {
            BackendError::Other(format!("invalid port {}: {e}", params.browser.port))
        })?;

        let name = pod_name("mica");

        // P3.5: owner label so Ingress controllers route session
        // proxy traffic back to the mica replica that holds the
        // SessionMap entry.
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "mica-session".to_string());
        labels.insert("mica/owner".to_string(), self.replica_id.clone());
        labels.insert("mica/session-name".to_string(), name.clone());
        for (k, v) in &params.browser.labels {
            labels.insert(format!("browser.{k}"), v.clone());
        }
        labels.insert(
            "mica/request-id".to_string(),
            // Kubernetes label values must be 63 chars max; truncate.
            params.request_id.chars().take(63).collect::<String>(),
        );

        // Env from browser + caps.
        let env: Vec<EnvVar> = params
            .browser
            .env
            .iter()
            .chain(params.caps.env.iter())
            .filter_map(|kv| {
                let mut split = kv.splitn(2, '=');
                Some(EnvVar {
                    name: split.next()?.to_string(),
                    value: Some(split.next().unwrap_or("").to_string()),
                    value_from: None,
                })
            })
            .collect();

        // P3.2: cpu / memory limits.
        let mut limits: BTreeMap<String, Quantity> = BTreeMap::new();
        if let Some(mem) = &params.browser.mem
            && !mem.is_empty()
        {
            limits.insert("memory".to_string(), Quantity(mem.clone()));
        }
        if let Some(cpu) = &params.browser.cpu
            && !cpu.is_empty()
        {
            limits.insert("cpu".to_string(), Quantity(cpu.clone()));
        }
        let resources = if limits.is_empty() {
            None
        } else {
            Some(ResourceRequirements {
                limits: Some(limits.clone()),
                requests: Some(limits),
                ..Default::default()
            })
        };

        // P3.2 + P2.4: emptyDir(medium=Memory) at /dev/shm sized 2 GiB
        // by default; per-browser shmSize wins.
        let shm_bytes = params
            .browser
            .shm_size
            .map(|n| n as i64)
            .unwrap_or(DEFAULT_SHM_BYTES);
        let mut volumes = vec![Volume {
            name: "dshm".into(),
            empty_dir: Some(EmptyDirVolumeSource {
                medium: Some("Memory".into()),
                size_limit: Some(Quantity(format!("{shm_bytes}"))),
            }),
            ..Default::default()
        }];
        let mut volume_mounts = vec![VolumeMount {
            name: "dshm".into(),
            mount_path: "/dev/shm".into(),
            ..Default::default()
        }];

        // P3.2: tmpfs entries -> additional emptyDir(medium=Memory) mounts.
        for (i, (path, size)) in params.browser.tmpfs.iter().enumerate() {
            let vname = format!("tmpfs-{i}");
            volumes.push(Volume {
                name: vname.clone(),
                empty_dir: Some(EmptyDirVolumeSource {
                    medium: Some("Memory".into()),
                    size_limit: Some(Quantity(size.clone())),
                }),
                ..Default::default()
            });
            volume_mounts.push(VolumeMount {
                name: vname,
                mount_path: path.clone(),
                ..Default::default()
            });
        }

        let mica_container = Container {
            name: "browser".into(),
            image: Some(image),
            ports: Some(vec![ContainerPort {
                container_port: webdriver_port,
                protocol: Some("TCP".into()),
                ..Default::default()
            }]),
            env: if env.is_empty() { None } else { Some(env) },
            resources,
            volume_mounts: Some(volume_mounts),
            ..Default::default()
        };

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![mica_container],
                volumes: Some(volumes),
                runtime_class_name: self.runtime_class.clone(),
                restart_policy: Some("Never".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let created = pods
            .create(&PostParams::default(), &pod)
            .await
            .map_err(|e| BackendError::Other(format!("create pod: {e}")))?;

        // P3.3: a headless Service per session — gives the proxy a
        // stable DNS name regardless of which node the Pod lands on.
        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.namespace);
        let svc = Service {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                cluster_ip: Some("None".into()), // headless
                selector: Some(labels.clone()),
                ports: Some(vec![ServicePort {
                    name: Some("webdriver".into()),
                    port: webdriver_port,
                    target_port: Some(
                        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                            webdriver_port,
                        ),
                    ),
                    protocol: Some("TCP".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let _ = services
            .create(&PostParams::default(), &svc)
            .await
            .map_err(|e| BackendError::Other(format!("create service: {e}")))?;

        // Wait for Pod readiness — poll until Ready or timeout.
        let pod_name = created
            .metadata
            .name
            .clone()
            .ok_or_else(|| BackendError::Other("pod name missing after create".into()))?;
        let deadline = Instant::now() + self.service_startup_timeout;
        loop {
            if Instant::now() >= deadline {
                let pods_for_cleanup = pods.clone();
                let services_for_cleanup = services.clone();
                let n = pod_name.clone();
                tokio::spawn(async move {
                    let _ = pods_for_cleanup.delete(&n, &DeleteParams::default()).await;
                    let _ = services_for_cleanup
                        .delete(&n, &DeleteParams::default())
                        .await;
                });
                return Err(BackendError::Timeout);
            }
            let p = pods.get(&pod_name).await.ok();
            if let Some(ready) = p
                .as_ref()
                .and_then(|p| p.status.as_ref())
                .and_then(|s| s.conditions.as_ref())
                .and_then(|cs| cs.iter().find(|c| c.type_ == "Ready"))
                && ready.status == "True"
            {
                break;
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }

        // Headless service DNS: <name>.<namespace>.svc.cluster.local
        let upstream = format!(
            "http://{name}.{ns}.svc.cluster.local:{webdriver_port}{path}",
            name = name,
            ns = self.namespace,
            path = params.browser.path.as_deref().unwrap_or(""),
        );

        Ok(StartedSession {
            upstream,
            container_id: name.clone(),
            // K8s pods don't expose host ports the way Docker does;
            // VNC / devtools / etc. are reached via the same headless
            // service DNS by adding more ServicePorts. Phase 3 ships
            // WebDriver only; Phase 4 microVM Kata flow opens the rest.
            host_ports: HostPorts::default(),
            started_at: SystemTime::now(),
            stopper: Box::new(K8sStopper {
                pods: Arc::new(pods),
                services: Arc::new(services),
                pod_name: name,
            }),
        })
    }
}

struct K8sStopper {
    pods: Arc<Api<Pod>>,
    services: Arc<Api<Service>>,
    pod_name: String,
}

#[async_trait]
impl Stopper for K8sStopper {
    async fn stop(self: Box<Self>) {
        let n = self.pod_name.clone();
        // Best-effort delete; swallow errors so a single API hiccup
        // can't abort downstream cancel-hook work.
        let _ = self.pods.delete(&n, &DeleteParams::default()).await;
        let _ = self.services.delete(&n, &DeleteParams::default()).await;
    }
}
