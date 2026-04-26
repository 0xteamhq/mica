//! Docker backend — `Backend` trait impl using `bollard`.
//!
//! Covers Phase 1 M7 tasks T20-T27:
//! - T20 connect / ping
//! - T21 start() happy path: pull-if-missing, create + start, wait for
//!   WebDriver TCP port to accept connections
//! - T22 port mapping for 5900 (VNC), 7070 (devtools), 8080 (fileserver),
//!   9090 (clipboard) — host ports surfaced via `HostPorts`
//! - T23 memory + CPU limits (per-browser overrides global)
//! - T24 tmpfs, volumes, env, sysctl, shm size from `browsers.json`
//! - T25 attach to a custom Docker network when `--container-network`
//!   != "default"
//! - T26 `DockerStopper`: SIGTERM -> 10s wait -> SIGKILL -> remove
//!   --force. Never leak containers.
//! - T27 image / start failures surface as `BackendError::Docker(msg)`
//!
//! Tests live in `tests/docker_integration.rs`, gated behind
//! `MICA_DOCKER_TESTS=1`.

use super::{Backend, BackendError, HostPorts, StartParams, StartedSession, Stopper};
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, HostConfigLogConfig, PortBinding};
use bollard::network::ConnectNetworkOptions;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::io::AsyncWriteExt;

const VNC_PORT: u16 = 5900;
const DEVTOOLS_PORT: u16 = 7070;
const FILESERVER_PORT: u16 = 8080;
const CLIPBOARD_PORT: u16 = 9090;

#[derive(Clone)]
pub struct DockerBackend {
    client: Docker,
    network: String,
    default_cpu: String,
    default_memory: String,
    service_startup_timeout: Duration,
    /// Stop timeout in seconds — sent to docker as the SIGTERM grace
    /// window before SIGKILL.
    stop_timeout_secs: i64,
    /// T50 — when set, every container's stdout/stderr stream is
    /// captured to `<save_all_logs_dir>/<container_id>.log`.
    save_all_logs_dir: Option<String>,
    /// Selenoid parity: privileged-by-default. When `true`, mica
    /// drops the privileged flag on every container it creates.
    disable_privileged: bool,
    /// Selenoid parity: optional `HostConfig.LogConfig` JSON parsed
    /// from the `--log-conf` CLI flag.
    log_config: Option<HostConfigLogConfig>,
}

impl DockerBackend {
    /// T20: connect to the local Docker daemon (unix socket or
    /// `DOCKER_HOST` env) with sensible defaults.
    pub async fn connect() -> Result<Self, BackendError> {
        let client = Docker::connect_with_local_defaults()
            .map_err(|e| BackendError::Docker(e.to_string()))?;
        Ok(Self {
            client,
            network: "default".into(),
            default_cpu: String::new(),
            default_memory: String::new(),
            service_startup_timeout: Duration::from_secs(30),
            stop_timeout_secs: 10,
            save_all_logs_dir: None,
            disable_privileged: false,
            log_config: None,
        })
    }

    /// Selenoid parity: turn off privileged mode for every container.
    pub fn with_disable_privileged(mut self, off: bool) -> Self {
        self.disable_privileged = off;
        self
    }

    /// Selenoid parity: parse `--log-conf` JSON into a `HostConfigLogConfig`.
    /// Format: `{"type":"json-file","config":{"max-size":"10m"}}`.
    pub fn with_log_conf(mut self, raw: &str) -> Self {
        if raw.is_empty() {
            return self;
        }
        match serde_json::from_str::<HostConfigLogConfig>(raw) {
            Ok(cfg) => {
                self.log_config = Some(cfg);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ignoring invalid --log-conf JSON");
            }
        }
        self
    }

    /// T50: capture container stdout/stderr into
    /// `<dir>/<container_id>.log`. Pass `None` to disable.
    pub fn with_save_all_logs(mut self, dir: Option<String>) -> Self {
        self.save_all_logs_dir = dir.filter(|d| !d.is_empty());
        self
    }

    pub fn with_network(mut self, n: impl Into<String>) -> Self {
        self.network = n.into();
        self
    }

    pub fn with_default_cpu(mut self, c: impl Into<String>) -> Self {
        self.default_cpu = c.into();
        self
    }

    pub fn with_default_memory(mut self, m: impl Into<String>) -> Self {
        self.default_memory = m.into();
        self
    }

    pub fn with_service_startup_timeout(mut self, d: Duration) -> Self {
        self.service_startup_timeout = d;
        self
    }

    pub async fn ping(&self) -> Result<(), BackendError> {
        self.client
            .ping()
            .await
            .map(|_| ())
            .map_err(|e| BackendError::Docker(e.to_string()))
    }

    fn merge_cpu(&self, browser_cpu: &Option<String>) -> Option<i64> {
        let raw = browser_cpu.clone().filter(|s| !s.is_empty()).or_else(|| {
            if self.default_cpu.is_empty() {
                None
            } else {
                Some(self.default_cpu.clone())
            }
        })?;
        // Selenoid accepts CPU as a fractional core count ("0.5") or an
        // integer ("2"). Bollard wants nanocpus (1e9 = 1 vCPU).
        raw.parse::<f64>().ok().map(|n| (n * 1e9) as i64)
    }

    fn merge_memory(&self, browser_mem: &Option<String>) -> Option<i64> {
        let raw = browser_mem.clone().filter(|s| !s.is_empty()).or_else(|| {
            if self.default_memory.is_empty() {
                None
            } else {
                Some(self.default_memory.clone())
            }
        })?;
        parse_memory(&raw)
    }
}

#[async_trait]
impl Backend for DockerBackend {
    async fn start(&self, params: StartParams) -> Result<StartedSession, BackendError> {
        let image = params
            .browser
            .docker_image()
            .ok_or_else(|| {
                BackendError::Docker("driver-mode image entries are unsupported".into())
            })?
            .to_string();

        // T27: pull-if-missing. We don't fail when the image already
        // exists — `inspect_image` is the cheap probe.
        if self.client.inspect_image(&image).await.is_err() {
            let mut stream = self.client.create_image(
                Some(CreateImageOptions::<String> {
                    from_image: image.clone(),
                    ..Default::default()
                }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                item.map_err(|e| BackendError::Docker(format!("pull {image} failed: {e}")))?;
            }
        }

        // T22: publish all four mica-relevant container ports to
        // ephemeral host ports.
        let webdriver_port: u16 = params.browser.port.parse().map_err(|e| {
            BackendError::Docker(format!("invalid port {}: {e}", params.browser.port))
        })?;
        let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();
        for p in [
            webdriver_port,
            VNC_PORT,
            DEVTOOLS_PORT,
            FILESERVER_PORT,
            CLIPBOARD_PORT,
        ] {
            let key = format!("{p}/tcp");
            port_bindings.insert(
                key.clone(),
                Some(vec![PortBinding {
                    host_ip: Some(String::new()),
                    host_port: Some(String::new()),
                }]),
            );
            exposed_ports.insert(key, HashMap::new());
        }

        // T24: tmpfs / volumes / env / sysctl / shm size from browsers.json.
        let browser = &params.browser;
        let caps = &params.caps;
        let tmpfs = if browser.tmpfs.is_empty() {
            None
        } else {
            Some(browser.tmpfs.clone())
        };
        let binds = if browser.volumes.is_empty() {
            None
        } else {
            Some(browser.volumes.clone())
        };
        let sysctls = if browser.sysctl.is_empty() {
            None
        } else {
            Some(browser.sysctl.clone())
        };
        // P2.4: Chromium's default /dev/shm of 64 MB is far too small
        // (browserless documents 2 GB as the safe floor). If the
        // operator hasn't set shmSize in browsers.json, default to 2
        // GiB so Chrome doesn't crash under load.
        const DEFAULT_SHM: i64 = 2 * 1024 * 1024 * 1024;
        let shm = Some(browser.shm_size.map(|n| n as i64).unwrap_or(DEFAULT_SHM));

        // T23: per-browser cpu/memory > global default.
        let memory = self.merge_memory(&browser.mem);
        let nano_cpus = self.merge_cpu(&browser.cpu);

        // T25: attach to a custom network when configured. We always
        // start with the default bridge; when the operator sets
        // --container-network, we attach the container to that network
        // post-create. That works both when the network exists and
        // surfaces a clean error when it doesn't (T25 acceptance).
        let network_mode = if self.network == "default" {
            None
        } else {
            // We'll attach explicitly below; leave NetworkMode at the
            // daemon default to avoid bollard's "network not found"
            // crashing during container create.
            None
        };

        // Selenoid parity: when `publishAllPorts` is set, drop our
        // explicit port_bindings and let docker map every exposed
        // port to a host-side ephemeral port.
        let (final_port_bindings, publish_all_ports) = if browser.publish_all_ports {
            (None, Some(true))
        } else {
            (Some(port_bindings), None)
        };
        // Selenoid parity: hosts_entries from caps merge with browsers.json hosts.
        let mut extra_hosts: Vec<String> = browser
            .hosts
            .iter()
            .cloned()
            .chain(caps.hosts_entries.iter().cloned())
            .collect();
        extra_hosts.sort();
        extra_hosts.dedup();
        let extra_hosts = if extra_hosts.is_empty() {
            None
        } else {
            Some(extra_hosts)
        };

        let dns = if caps.dns_servers.is_empty() {
            None
        } else {
            Some(caps.dns_servers.clone())
        };

        // Selenoid parity: applicationContainers becomes HostConfig.Links.
        let links = if caps.application_containers.is_empty() {
            None
        } else {
            Some(caps.application_containers.clone())
        };

        let host_config = HostConfig {
            port_bindings: final_port_bindings,
            publish_all_ports,
            memory,
            nano_cpus,
            tmpfs,
            binds,
            shm_size: shm,
            sysctls,
            network_mode,
            extra_hosts,
            dns,
            links,
            privileged: Some(!self.disable_privileged),
            log_config: self.log_config.clone(),
            ..Default::default()
        };

        // Selenoid parity (service/docker.go:359-374): mica auto-injects
        // standard env vars FIRST so browser.env and caps.env can override.
        // Order: [auto-injected] -> browser.env -> caps.env (last wins).
        let mut env: Vec<String> = Vec::new();
        if let Some(tz) = &caps.time_zone
            && !tz.is_empty()
        {
            env.push(format!("TZ={tz}"));
        }
        let video_size = caps
            .video_screen_size
            .clone()
            .or_else(|| caps.screen_resolution.clone());
        if let Some(res) = &caps.screen_resolution
            && !res.is_empty()
        {
            env.push(format!("SCREEN_RESOLUTION={res}"));
        }
        if caps.enable_vnc {
            env.push("ENABLE_VNC=true".to_string());
        }
        if caps.enable_video {
            env.push("ENABLE_VIDEO=true".to_string());
        }
        if let Some(size) = &video_size {
            env.push(format!("VIDEO_SIZE={size}"));
        }
        if let Some(codec) = &caps.video_codec
            && !codec.is_empty()
        {
            env.push(format!("CODEC={codec}"));
        }
        if let Some(fr) = caps.video_frame_rate {
            env.push(format!("VIDEO_FRAME_RATE={fr}"));
        }
        env.extend(browser.env.iter().cloned());
        env.extend(caps.env.iter().cloned());

        let mut labels: HashMap<String, String> = browser
            .labels
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .chain([
                ("mica.session.request_id".into(), params.request_id.clone()),
                ("mica.browser.version".into(), params.version.clone()),
            ])
            .collect();
        // Selenoid parity (service/docker.go:423-437): test-name label.
        if let Some(name) = &caps.name
            && !name.is_empty()
        {
            labels.insert("name".into(), name.clone());
        }

        let cfg = ContainerConfig::<String> {
            image: Some(image.clone()),
            hostname: caps.container_hostname.clone().filter(|s| !s.is_empty()),
            env: if env.is_empty() { None } else { Some(env) },
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            labels: Some(labels),
            ..Default::default()
        };

        // Create
        let create = self
            .client
            .create_container(None::<CreateContainerOptions<String>>, cfg)
            .await
            .map_err(|e| BackendError::Docker(format!("create {image}: {e}")))?;
        let id = create.id;

        // T25: attach to the operator-configured custom network before start.
        if self.network != "default" {
            self.client
                .connect_network(
                    &self.network,
                    ConnectNetworkOptions {
                        container: id.clone(),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| {
                    BackendError::Docker(format!("attach network {}: {e}", self.network))
                })?;
        }
        // Selenoid parity: additionalNetworks from caps — also attached
        // post-create. Each is independent of --container-network.
        for net in &caps.additional_networks {
            if net.is_empty() {
                continue;
            }
            self.client
                .connect_network(
                    net,
                    ConnectNetworkOptions {
                        container: id.clone(),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| {
                    BackendError::Docker(format!("attach additional network {net}: {e}"))
                })?;
        }

        // Start
        self.client
            .start_container(&id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| BackendError::Docker(format!("start {id}: {e}")))?;

        // Inspect to read the assigned host ports.
        let inspect = self
            .client
            .inspect_container(&id, None)
            .await
            .map_err(|e| BackendError::Docker(format!("inspect {id}: {e}")))?;

        let host_port_for = |container_port: u16| -> Option<String> {
            let key = format!("{container_port}/tcp");
            inspect
                .network_settings
                .as_ref()?
                .ports
                .as_ref()?
                .get(&key)
                .and_then(|opt| opt.as_ref())
                .and_then(|bindings| bindings.first())
                .and_then(|b| b.host_port.clone())
        };

        let webdriver_host_port = host_port_for(webdriver_port).ok_or_else(|| {
            BackendError::Docker(format!("no host port mapped for {webdriver_port}/tcp"))
        })?;

        let host_ports = HostPorts {
            vnc: host_port_for(VNC_PORT),
            devtools: host_port_for(DEVTOOLS_PORT),
            fileserver: host_port_for(FILESERVER_PORT),
            clipboard: host_port_for(CLIPBOARD_PORT),
        };

        // T50: stream container logs to file when --save-all-logs.
        if let Some(dir) = &self.save_all_logs_dir {
            let _ = tokio::fs::create_dir_all(dir).await;
            let path = format!("{dir}/{id}.log");
            let client = self.client.clone();
            let id = id.clone();
            tokio::spawn(async move {
                let mut stream = client.logs(
                    &id,
                    Some(LogsOptions::<String> {
                        follow: true,
                        stdout: true,
                        stderr: true,
                        ..Default::default()
                    }),
                );
                let mut file = match tokio::fs::File::create(&path).await {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(error = %e, %path, "open log file");
                        return;
                    }
                };
                while let Some(item) = stream.next().await {
                    let bytes = match item {
                        Ok(LogOutput::StdOut { message } | LogOutput::StdErr { message }) => {
                            message
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    };
                    if file.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
            });
        }

        // T21: wait for the WebDriver port to accept TCP within
        // service_startup_timeout. 100 ms backoff.
        wait_for_tcp(
            "127.0.0.1",
            &webdriver_host_port,
            self.service_startup_timeout,
        )
        .await
        .map_err(|_| {
            // best-effort cleanup so we don't leak a half-booted container
            let client = self.client.clone();
            let id_clone = id.clone();
            tokio::spawn(async move {
                let _ = client
                    .remove_container(
                        &id_clone,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
            });
            BackendError::Timeout
        })?;

        let upstream = match browser.path.as_deref() {
            Some(p) if !p.is_empty() => format!("http://127.0.0.1:{webdriver_host_port}{p}"),
            _ => format!("http://127.0.0.1:{webdriver_host_port}"),
        };

        Ok(StartedSession {
            upstream,
            container_id: id.clone(),
            host_ports,
            started_at: SystemTime::now(),
            stopper: Box::new(DockerStopper {
                client: Arc::new(self.client.clone()),
                container_id: id,
                stop_timeout_secs: self.stop_timeout_secs,
            }),
        })
    }
}

/// T26: SIGTERM (with timeout) -> SIGKILL -> remove --force. Always
/// removes the container at the end so we never leak on panic.
struct DockerStopper {
    client: Arc<Docker>,
    container_id: String,
    stop_timeout_secs: i64,
}

#[async_trait]
impl Stopper for DockerStopper {
    async fn stop(self: Box<Self>) {
        let id = self.container_id.clone();
        // SIGTERM with a stop_timeout window. Bollard then sends SIGKILL
        // automatically when t expires.
        let _ = self
            .client
            .stop_container(
                &id,
                Some(StopContainerOptions {
                    t: self.stop_timeout_secs,
                }),
            )
            .await;
        let _ = self
            .client
            .remove_container(
                &id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
    }
}

/// Poll `host:port` until a TCP handshake succeeds or `timeout` elapses.
async fn wait_for_tcp(host: &str, port: &str, timeout: Duration) -> Result<(), ()> {
    let addr = format!("{host}:{port}");
    let deadline = Instant::now() + timeout;
    loop {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Parse Selenoid-style memory strings: "256m", "1g", "512M", "1024" (bytes).
fn parse_memory(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, suffix) = s.split_at(s.len() - 1);
    match suffix {
        "k" | "K" => num.parse::<i64>().ok().map(|n| n * 1024),
        "m" | "M" => num.parse::<i64>().ok().map(|n| n * 1024 * 1024),
        "g" | "G" => num.parse::<i64>().ok().map(|n| n * 1024 * 1024 * 1024),
        _ => s.parse::<i64>().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_memory;

    #[test]
    fn parses_memory_suffixes() {
        assert_eq!(parse_memory("256m"), Some(256 * 1024 * 1024));
        assert_eq!(parse_memory("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory("4096"), Some(4096));
        assert_eq!(parse_memory(""), None);
    }
}
