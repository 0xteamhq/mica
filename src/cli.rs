use clap::Parser;
use std::time::Duration;

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

#[derive(Parser, Debug, Clone)]
#[command(name = "mica", version, about = "Browser grid for the T-system")]
pub struct Args {
    /// Listen address.
    #[arg(long, default_value = ":4444", env = "MICA_LISTEN")]
    pub listen: String,

    /// Path to browsers.json.
    #[arg(long, default_value = "config/browsers.json", env = "MICA_CONF")]
    pub conf: String,

    /// Max parallel sessions.
    #[arg(long, default_value_t = 5)]
    pub limit: u32,

    /// Session idle timeout.
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    pub timeout: Duration,

    /// Maximum valid session idle timeout that a client can request via
    /// `mica:options.sessionTimeout`. Caps that ask for more are
    /// clamped to this.
    #[arg(long, default_value = "1h", value_parser = parse_duration)]
    pub max_timeout: Duration,

    /// Service startup timeout.
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    pub service_startup_timeout: Duration,

    /// New-session attempt timeout.
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    pub session_attempt_timeout: Duration,

    /// Timeout for the upstream `DELETE /session/{id}` call we make
    /// from the cancel hook (idle / DELETE / shutdown teardown paths).
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    pub session_delete_timeout: Duration,

    /// Number of new-session retries.
    #[arg(long, default_value_t = 1)]
    pub retry_count: u32,

    /// Video output directory.
    #[arg(long, default_value = "video")]
    pub video_output_dir: String,

    /// Log output directory.
    #[arg(long, default_value = "logs")]
    pub log_output_dir: String,

    /// Container network.
    #[arg(long, default_value = "default")]
    pub container_network: String,

    /// Default CPU limit (empty = unlimited).
    #[arg(long, default_value = "")]
    pub cpu: String,

    /// Default memory limit (empty = unlimited).
    #[arg(long, default_value = "")]
    pub memory: String,

    /// Enable /file upload endpoint.
    #[arg(long, default_value_t = false)]
    pub enable_file_upload: bool,

    /// Disable the request queue: instead of blocking until a slot frees
    /// up, fail the create immediately with the W3C error
    /// `session not created` / `queue is full`.
    // The HTTP status for that error comes from the usual `WdError`
    // `IntoResponse` mapping in `error.rs` (500 today). Kept out of the doc
    // comment so it stays out of `--help`.
    #[arg(long, default_value_t = false)]
    pub disable_queue: bool,

    /// Graceful shutdown period.
    #[arg(long, default_value = "300s", value_parser = parse_duration)]
    pub graceful_period: Duration,

    /// Save all container logs (not only sessions with `enableLog`).
    #[arg(long, default_value_t = false)]
    pub save_all_logs: bool,

    /// Disable privileged container mode. mica is `false` by
    /// default; this flag re-enables that strict policy. Phase-4
    /// isolation drivers override it when they require it (e.g. KVM
    /// device access).
    #[arg(long, default_value_t = false)]
    pub disable_privileged: bool,

    /// Container log config as JSON, e.g.
    /// `{"type":"json-file","config":{"max-size":"10m"}}`. Maps to
    /// Docker's `HostConfig.LogConfig`.
    #[arg(long, default_value = "", env = "MICA_LOG_CONF")]
    pub log_conf: String,

    /// S3 bucket for artifact upload. Empty = upload disabled.
    #[arg(long, default_value = "", env = "MICA_S3_BUCKET")]
    pub s3_bucket: String,

    /// S3 region (defaults to the AWS SDK default chain when empty).
    #[arg(long, default_value = "", env = "MICA_S3_REGION")]
    pub s3_region: String,

    /// S3 key prefix prepended to every uploaded object.
    #[arg(long, default_value = "", env = "MICA_S3_PREFIX")]
    pub s3_prefix: String,

    /// Force path-style S3 addressing (`endpoint/bucket/key` instead of
    /// the virtual-host `bucket.endpoint/key`). Required for
    /// S3-compatible stores like MinIO / Ceph. The endpoint URL itself
    /// is set via the standard `AWS_ENDPOINT_URL_S3` env var.
    #[arg(long, default_value_t = false, env = "MICA_S3_FORCE_PATH_STYLE")]
    pub s3_force_path_style: bool,

    /// Min idle warm-pool size per browser key. 0 disables the pool.
    #[arg(long, default_value_t = 0)]
    pub warm_pool_min: u32,

    /// Max idle warm-pool size per browser key.
    #[arg(long, default_value_t = 16)]
    pub warm_pool_max: u32,

    /// Idle TTL: pool entries older than this are evicted.
    #[arg(long, default_value = "5m", value_parser = parse_duration)]
    pub warm_pool_idle_ttl: Duration,

    /// Backend driver: "docker" (default) or "k8s".
    #[arg(long, default_value = "docker", env = "MICA_BACKEND")]
    pub backend: String,

    /// Kubernetes namespace (when --backend=k8s).
    #[arg(long, default_value = "default", env = "MICA_K8S_NAMESPACE")]
    pub k8s_namespace: String,

    /// Kubernetes RuntimeClass name to set on session pods. Set to
    /// "kata" for VM-grade isolation (Phase 4) or "gvisor" for the
    /// stock-managed-K8s sandbox path. Empty = node default.
    #[arg(long, default_value = "", env = "MICA_K8S_RUNTIME_CLASS")]
    pub k8s_runtime_class: String,

    /// Replica id stamped on every Pod's `mica/owner=<id>` label. Empty
    /// = a per-process UUID. Set this from a downward-API field in
    /// multi-replica deployments so Ingress can stick to the owning
    /// mica replica.
    #[arg(long, default_value = "", env = "MICA_REPLICA_ID")]
    pub replica_id: String,

    /// Isolation driver. `auto` picks the strongest one the host
    /// supports (KVM > runsc > runc). Explicit pins: runc | gvisor
    /// | kata | firecracker | cloud_hypervisor.
    #[arg(long, default_value = "auto", env = "MICA_ISOLATION")]
    pub isolation: String,

    /// Directory of `.wasm` plugin components. Empty = no plugins.
    #[arg(long, default_value = "", env = "MICA_PLUGIN_DIR")]
    pub plugin_dir: String,

    /// Per-plugin capability grants. Comma-separated `<plugin>=<cap>[,<cap>...]`
    /// pairs joined by `;`. Example:
    /// `--plugin-grants gcs=http-client,state;auth=http-client`.
    /// Capabilities: `host-log` and `clock` are always granted;
    /// the gated set is `http-client`, `s3-write`, `state`. A plugin
    /// importing a capability not in its grant set fails to
    /// instantiate at startup with a clear error.
    #[arg(long, default_value = "", env = "MICA_PLUGIN_GRANTS")]
    pub plugin_grants: String,

    /// Directory backing the plugin `state` capability. When unset,
    /// plugins granted `state` get their own scratch dir under
    /// `${TMPDIR}/mica-plugin-state` so the binary still runs without
    /// operator setup; production deployments should pass a durable
    /// path so plugin state survives mica restarts.
    #[arg(long, default_value = "", env = "MICA_PLUGIN_STATE_DIR")]
    pub plugin_state_dir: String,

    /// Total time budget across the entire session.on-create plugin
    /// chain. Exceeding it is treated as transient; mica returns
    /// `session not created` to the client. 0 disables the timeout.
    #[arg(long, default_value = "5s", value_parser = parse_duration)]
    pub plugin_on_create_timeout: Duration,

    /// Per-plugin time budget for `lifecycle.shutdown` during
    /// graceful shutdown. Plugins exceeding it are dropped without
    /// completing flush; mica logs and continues exiting.
    #[arg(long, default_value = "5s", value_parser = parse_duration)]
    pub plugin_shutdown_timeout: Duration,

    /// Per-plugin configuration TOML. Each `[plugins.<name>]` table
    /// is flattened to `list<header>` and passed to that plugin's
    /// `lifecycle.init`. Only string-, integer-, float- and bool-
    /// scalar values are honored; nested tables produce a warning
    /// and are skipped. Empty path = no per-plugin config (init
    /// receives an empty header list).
    #[arg(long, default_value = "", env = "MICA_PLUGIN_CONFIG")]
    pub plugin_config: String,

    /// Per-call timeout applied to each plugin's
    /// `http.intercept_request` and `http.intercept_response`. This
    /// is the proxy hot path; a slow plugin would block every
    /// WebDriver action, so the budget is small by default.
    #[arg(long, default_value = "500ms", value_parser = parse_duration)]
    pub plugin_http_timeout: Duration,

    /// htpasswd file gating every WebDriver / VNC / artifact / relay
    /// endpoint with HTTP Basic auth. Empty = open. Reloaded on
    /// SIGHUP alongside browsers.json. Health (/healthz, /readyz),
    /// liveness (/ping), metrics (/metrics), and the OpenAPI spec
    /// (/openapi.yaml) stay open so K8s probes and Prometheus
    /// scrapers don't need credentials.
    #[arg(long, default_value = "", env = "MICA_USERS")]
    pub users: String,

    /// Per-user session quota file (M3), JSON:
    /// `{"default": 0, "users": {"alice": 3}}` — 0 = unlimited.
    /// Checked before the global queue so an over-quota create fails
    /// fast without burning a slot. Empty = quotas disabled. Reloaded
    /// on SIGHUP and editable via PUT /admin/api/quotas. Applies only
    /// to authenticated sessions (open mode has no owner to meter).
    #[arg(long, default_value = "", env = "MICA_QUOTAS")]
    pub quotas: String,

    /// Run as a stateless router (GGR equivalent) in front of N mica
    /// nodes instead of a grid node. Requires --nodes. Backend and
    /// isolation flags are ignored; docker/k8s are never touched.
    /// Session ids returned to clients embed the node name
    /// (`base64url(name).upstream_id`) so any router replica can
    /// route any request without shared state.
    #[arg(long, default_value_t = false, env = "MICA_ROUTER")]
    pub router: bool,

    /// Node registry for router mode (nodes.json). Hot-reloaded on
    /// SIGHUP. Unreadable/invalid at startup is a hard error — a
    /// router with no nodes serves nothing.
    #[arg(long, default_value = "config/nodes.json", env = "MICA_NODES")]
    pub nodes: String,

    /// How often the router polls each node's /status for health +
    /// capability data (router mode).
    #[arg(long, default_value = "5s", value_parser = parse_duration, env = "MICA_ROUTER_HEALTH_INTERVAL")]
    pub router_health_interval: Duration,

    /// Per-poll timeout for a node /status request (router mode).
    #[arg(long, default_value = "5s", value_parser = parse_duration, env = "MICA_ROUTER_HEALTH_TIMEOUT")]
    pub router_health_timeout: Duration,

    /// Consecutive failed polls before a node is marked unhealthy and
    /// excluded from new-session placement (router mode). Existing
    /// sessions are still proxied to it.
    #[arg(long, default_value_t = 2, env = "MICA_ROUTER_UNHEALTHY_THRESHOLD")]
    pub router_unhealthy_threshold: u32,

    /// Max distinct nodes tried per create-session request before
    /// giving up (router mode).
    #[arg(long, default_value_t = 3, env = "MICA_ROUTER_MAX_ATTEMPTS")]
    pub router_max_attempts: u32,

    /// Per-attempt timeout for a forwarded create (router mode).
    /// Deliberately long: the node may queue the request.
    #[arg(long, default_value = "5m", value_parser = parse_duration, env = "MICA_ROUTER_CREATE_TIMEOUT")]
    pub router_create_timeout: Duration,
}
