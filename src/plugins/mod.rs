//! WASM plugin host (P5.1, P5.2, P5.3).
//!
//! `wasmtime` 26 + WASI Preview 2 + Component Model. Contract lives
//! in `wit/` (`world plugin` from `world.wit`).
//!
//! Scope shipped here:
//! - Loads every `.wasm` in `--plugin-dir`.
//! - Implements the always-granted host imports (`mica:plugin/host-log`
//!   and `mica:plugin/clock`).
//! - Calls `lifecycle.init` on each plugin once at startup.
//! - `artifact_verdict()` runs the plugin chain over a `FileCreated`
//!   and returns an `ArtifactVerdict`. Plugins are called in load
//!   order; first non-`Keep` wins; subsequent plugins are skipped
//!   for that artifact (matches `wit/artifact.wit`). Caller acts on
//!   the verdict — `Keep` falls through to the built-in
//!   `S3Uploader`; `Skip` deletes the local file; `S3{...}` and
//!   `CustomUri` log + suppress the default uploader.
//!
//! Out of scope (next plugin commit):
//! - Capability-gated imports (`http-client`, `s3-write`, `state`)
//!   plus the `--plugin-grants <name>=<caps>` CLI surface.
//! - `session.on-create` (caps rewrite + reject) and the http
//!   middleware hooks. Each needs handler-side wiring.
//! - Per-plugin config from `--plugin-config <toml>`. Today
//!   `lifecycle.init` is called with an empty config record — name +
//!   empty version + no headers.

use anyhow::Context;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use wasmtime::Engine;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

use crate::events::{ArtifactKind, FileCreated};

/// Set of capabilities a plugin can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    HttpClient,
    S3Write,
    State,
}

impl Capability {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "http-client" => Some(Self::HttpClient),
            "s3-write" => Some(Self::S3Write),
            "state" => Some(Self::State),
            _ => None,
        }
    }
}

/// Per-plugin grant table parsed from `--plugin-grants`.
///
/// Format: `<plugin>=<cap>[,<cap>...]` pairs separated by `;`.
/// Example:
///   `gcs=http-client,state;auth=http-client`
#[derive(Debug, Clone, Default)]
pub struct GrantTable {
    inner: HashMap<String, HashSet<Capability>>,
}

impl GrantTable {
    pub fn parse(s: &str) -> Self {
        let mut inner: HashMap<String, HashSet<Capability>> = HashMap::new();
        if s.is_empty() {
            return Self { inner };
        }
        for entry in s.split(';') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let Some((name, caps)) = entry.split_once('=') else {
                tracing::warn!(entry, "ignoring malformed plugin-grants entry");
                continue;
            };
            let set = inner.entry(name.trim().to_string()).or_default();
            for cap in caps.split(',') {
                match Capability::parse(cap) {
                    Some(c) => {
                        set.insert(c);
                    }
                    None => {
                        tracing::warn!(plugin = %name, cap = %cap, "unknown capability ignored")
                    }
                }
            }
        }
        Self { inner }
    }

    pub fn for_plugin(&self, name: &str) -> HashSet<Capability> {
        self.inner.get(name).cloned().unwrap_or_default()
    }
}

mod bindings {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "wit",
        async: true,
    });
}

use bindings::Plugin as PluginInstance;
use bindings::exports::mica::plugin::artifact::UploadDestination;
use bindings::exports::mica::plugin::lifecycle::Config as WitConfig;
use bindings::mica::plugin::host_log::Level as HostLogLevel;
use bindings::mica::plugin::types::{
    ArtifactKind as WitArtifactKind, FileInfo as WitFileInfo, HttpRequest as WitHttpRequest,
    HttpResponse as WitHttpResponse, Instant as WitInstant, PluginError as WitPluginError,
};

/// Resolution of the artifact-handler chain. Cancel-hook callers
/// switch on this to decide what to do with the on-disk artifact.
#[derive(Debug, Clone)]
pub enum ArtifactVerdict {
    /// All plugins returned `Keep` (or the host has no plugins).
    /// Caller falls through to the built-in `S3Uploader`.
    Keep,
    /// A plugin asked mica to drop the artifact. Caller should
    /// delete the local file and NOT run the default uploader.
    Skip,
    /// A plugin specified an explicit S3 destination. mica logs
    /// the directive but does not perform the upload itself —
    /// plugins requiring this path implement the transfer via
    /// the `s3-write` capability (granted via `--plugin-grants`).
    /// Default uploader is suppressed.
    S3 {
        bucket: String,
        key: String,
        region: Option<String>,
    },
    /// Plugin handled the artifact via an out-of-band URI.
    /// Default uploader is suppressed.
    CustomUri(String),
}

pub struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
    /// Logical plugin name — surfaced in host-log records so plugin
    /// log lines automatically carry the plugin's identity.
    plugin_name: String,
    /// Shared HTTP client for plugins granted the `http-client`
    /// capability. None when not granted (the WIT method is
    /// unreachable since the host import isn't linked).
    http: Option<reqwest::Client>,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

// host-log: always granted. Routes plugin log records into mica's
// tracing subscriber, tagged with the plugin name.
#[async_trait]
impl bindings::mica::plugin::host_log::Host for HostState {
    async fn log(&mut self, level: HostLogLevel, message: String) {
        let plugin = self.plugin_name.as_str();
        match level {
            HostLogLevel::Trace => tracing::trace!(plugin, "{}", message),
            HostLogLevel::Debug => tracing::debug!(plugin, "{}", message),
            HostLogLevel::Info => tracing::info!(plugin, "{}", message),
            HostLogLevel::Warn => tracing::warn!(plugin, "{}", message),
            HostLogLevel::Error => tracing::error!(plugin, "{}", message),
        }
    }
}

// clock: always granted. Returns wall-clock time.
#[async_trait]
impl bindings::mica::plugin::clock::Host for HostState {
    async fn now(&mut self) -> WitInstant {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => WitInstant {
                seconds: d.as_secs(),
                nanos: d.subsec_nanos(),
            },
            Err(_) => WitInstant {
                seconds: 0,
                nanos: 0,
            },
        }
    }
}

// http-client: capability-gated. Linked into a plugin's linker only
// when `--plugin-grants <name>=http-client` includes this plugin.
// The host runs the request via reqwest with a 30 s timeout. Mica
// resolves DNS in-host; the plugin sandbox cannot open raw sockets,
// matching wit/host.wit's documented contract.
#[async_trait]
impl bindings::mica::plugin::http_client::Host for HostState {
    async fn send(&mut self, req: WitHttpRequest) -> Result<WitHttpResponse, WitPluginError> {
        let client = self.http.as_ref().ok_or_else(|| WitPluginError {
            code: "no-http-client".into(),
            message: "http-client not granted to this plugin".into(),
            transient: false,
        })?;
        let method =
            reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|e| WitPluginError {
                code: "bad-method".into(),
                message: format!("{e}"),
                transient: false,
            })?;
        let url = req.path.clone();
        let mut builder = client
            .request(method, &url)
            .timeout(Duration::from_secs(30));
        for h in &req.headers {
            builder = builder.header(&h.name, &h.value);
        }
        if !req.body.is_empty() {
            builder = builder.body(req.body.clone());
        }
        let resp = builder.send().await.map_err(|e| WitPluginError {
            code: "http-send".into(),
            message: format!("{e}"),
            transient: e.is_timeout() || e.is_connect(),
        })?;
        let status = resp.status().as_u16();
        let headers: Vec<bindings::mica::plugin::types::Header> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|s| bindings::mica::plugin::types::Header {
                        name: k.as_str().to_string(),
                        value: s.to_string(),
                    })
            })
            .collect();
        let body = resp
            .bytes()
            .await
            .map_err(|e| WitPluginError {
                code: "http-body".into(),
                message: format!("{e}"),
                transient: false,
            })?
            .to_vec();
        Ok(WitHttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[allow(dead_code)]
pub struct Plugin {
    pub name: String,
    pub(crate) component: Component,
}

#[derive(Clone)]
pub struct PluginHost {
    engine: Engine,
    plugins: Arc<Mutex<Vec<Plugin>>>,
    grants: Arc<GrantTable>,
    http: reqwest::Client,
}

impl PluginHost {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_grants(GrantTable::default())
    }

    pub fn with_grants(grants: GrantTable) -> anyhow::Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config).context("wasmtime engine")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("plugin http client")?;
        Ok(Self {
            engine,
            plugins: Arc::new(Mutex::new(Vec::new())),
            grants: Arc::new(grants),
            http,
        })
    }

    pub async fn load_dir(&self, dir: &Path) -> anyhow::Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wasm") {
                continue;
            }
            match self.load_one(&path).await {
                Ok(name) => tracing::info!(plugin = %name, path = %path.display(), "plugin loaded"),
                Err(e) => tracing::warn!(error = %e, path = %path.display(), "plugin load failed"),
            }
        }
        Ok(())
    }

    async fn load_one(&self, path: &PathBuf) -> anyhow::Result<String> {
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let component = Component::from_binary(&self.engine, &bytes)
            .with_context(|| format!("parse component {}", path.display()))?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("plugin")
            .to_string();
        self.plugins.lock().await.push(Plugin {
            name: name.clone(),
            component,
        });
        Ok(name)
    }

    pub async fn loaded_names(&self) -> Vec<String> {
        self.plugins
            .lock()
            .await
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    fn build_linker_for(&self, plugin_name: &str) -> anyhow::Result<Linker<HostState>> {
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker).context("wasi-preview2 linker")?;
        // Always-granted host imports.
        bindings::mica::plugin::host_log::add_to_linker(&mut linker, |s: &mut HostState| s)
            .context("host-log link")?;
        bindings::mica::plugin::clock::add_to_linker(&mut linker, |s: &mut HostState| s)
            .context("clock link")?;
        // Capability-gated imports. Linked only when the operator
        // grants the matching capability via --plugin-grants. A
        // plugin importing an ungranted capability fails to
        // instantiate with a clear "unknown import" error.
        let granted = self.grants.for_plugin(plugin_name);
        if granted.contains(&Capability::HttpClient) {
            bindings::mica::plugin::http_client::add_to_linker(&mut linker, |s: &mut HostState| s)
                .context("http-client link")?;
        }
        Ok(linker)
    }

    fn fresh_store(&self, plugin_name: &str) -> wasmtime::Store<HostState> {
        let table = ResourceTable::new();
        let wasi = WasiCtxBuilder::new().build();
        let granted = self.grants.for_plugin(plugin_name);
        let http = if granted.contains(&Capability::HttpClient) {
            Some(self.http.clone())
        } else {
            None
        };
        wasmtime::Store::new(
            &self.engine,
            HostState {
                table,
                wasi,
                plugin_name: plugin_name.to_string(),
                http,
            },
        )
    }

    /// Call `lifecycle.init` once on every loaded plugin.
    pub async fn init_all(&self) {
        let plugins = self.plugins.lock().await;
        if plugins.is_empty() {
            return;
        }
        for plugin in plugins.iter() {
            let linker = match self.build_linker_for(&plugin.name) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(plugin = %plugin.name, error = %e, "linker setup failed; skipping");
                    continue;
                }
            };
            let mut store = self.fresh_store(&plugin.name);
            let inst = match PluginInstance::instantiate_async(
                &mut store,
                &plugin.component,
                &linker,
            )
            .await
            {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!(plugin = %plugin.name, error = %e, "plugin instantiate failed (likely missing capability grant)");
                    continue;
                }
            };
            let cfg = WitConfig {
                name: plugin.name.clone(),
                version: String::new(),
                config: Vec::new(),
            };
            match inst
                .mica_plugin_lifecycle()
                .call_init(&mut store, &cfg)
                .await
            {
                Ok(Ok(())) => {
                    tracing::info!(plugin = %plugin.name, "plugin lifecycle.init ok");
                }
                Ok(Err(err)) => {
                    tracing::warn!(plugin = %plugin.name, code = %err.code, message = %err.message, transient = err.transient, "plugin lifecycle.init returned error");
                }
                Err(e) => {
                    tracing::warn!(plugin = %plugin.name, error = %e, "plugin lifecycle.init wasm trap");
                }
            }
        }
    }

    /// Run the plugin chain over a `FileCreated` and resolve to an
    /// `ArtifactVerdict`. Plugins are called in load order; first
    /// non-`Keep` wins. Errors / wasm traps in a plugin are treated
    /// as `Keep` for that plugin (try the next one) so a misbehaving
    /// plugin can't strand the chain.
    pub async fn artifact_verdict(&self, e: &FileCreated) -> ArtifactVerdict {
        let plugins = self.plugins.lock().await;
        if plugins.is_empty() {
            return ArtifactVerdict::Keep;
        }
        let info = WitFileInfo {
            path: e.path.to_string_lossy().to_string(),
            session_id: e.session_id.clone(),
            kind: match e.kind {
                ArtifactKind::Video => WitArtifactKind::Video,
                ArtifactKind::Log => WitArtifactKind::Log,
            },
            size_bytes: tokio::fs::metadata(&e.path)
                .await
                .map(|m| m.len())
                .unwrap_or(0),
        };
        for plugin in plugins.iter() {
            let linker = match self.build_linker_for(&plugin.name) {
                Ok(l) => l,
                Err(err) => {
                    tracing::warn!(plugin = %plugin.name, error = %err, "linker setup failed; skipping");
                    continue;
                }
            };
            let mut store = self.fresh_store(&plugin.name);
            let inst = match PluginInstance::instantiate_async(
                &mut store,
                &plugin.component,
                &linker,
            )
            .await
            {
                Ok(i) => i,
                Err(err) => {
                    tracing::warn!(plugin = %plugin.name, error = %err, "plugin instantiate failed during dispatch");
                    continue;
                }
            };
            let result = inst
                .mica_plugin_artifact()
                .call_on_file_created(&mut store, &info)
                .await;
            match result {
                Ok(Ok(UploadDestination::Keep)) => {
                    tracing::debug!(plugin = %plugin.name, path = %info.path, "plugin returned keep");
                }
                Ok(Ok(UploadDestination::Skip)) => {
                    tracing::info!(plugin = %plugin.name, path = %info.path, "plugin requested skip");
                    return ArtifactVerdict::Skip;
                }
                Ok(Ok(UploadDestination::S3(t))) => {
                    tracing::info!(plugin = %plugin.name, bucket = %t.bucket, key = %t.key, "plugin requested s3 destination");
                    return ArtifactVerdict::S3 {
                        bucket: t.bucket,
                        key: t.key,
                        region: t.region,
                    };
                }
                Ok(Ok(UploadDestination::CustomUri(uri))) => {
                    tracing::info!(plugin = %plugin.name, %uri, "plugin handled artifact via custom-uri");
                    return ArtifactVerdict::CustomUri(uri);
                }
                Ok(Err(err)) => {
                    tracing::warn!(plugin = %plugin.name, code = %err.code, message = %err.message, "plugin artifact.on_file_created returned error; treating as keep");
                }
                Err(err) => {
                    tracing::warn!(plugin = %plugin.name, error = %err, "plugin artifact.on_file_created wasm trap; treating as keep");
                }
            }
        }
        ArtifactVerdict::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn host_constructs_and_loads_empty_dir() {
        let host = PluginHost::new().expect("engine");
        let tmp = tempfile::tempdir().unwrap();
        host.load_dir(tmp.path()).await.expect("empty dir is ok");
        assert!(host.loaded_names().await.is_empty());
    }

    #[tokio::test]
    async fn init_all_on_empty_host_is_noop() {
        let host = PluginHost::new().expect("engine");
        host.init_all().await;
    }

    #[test]
    fn grant_table_empty_string_yields_empty() {
        let g = GrantTable::parse("");
        assert!(g.for_plugin("anyone").is_empty());
    }

    #[test]
    fn grant_table_parses_single_plugin_with_caps() {
        let g = GrantTable::parse("gcs=http-client,state");
        let caps = g.for_plugin("gcs");
        assert!(caps.contains(&Capability::HttpClient));
        assert!(caps.contains(&Capability::State));
        assert!(!caps.contains(&Capability::S3Write));
    }

    #[test]
    fn grant_table_parses_multiple_plugins() {
        let g = GrantTable::parse("gcs=http-client,s3-write;auth=http-client");
        assert!(g.for_plugin("gcs").contains(&Capability::S3Write));
        assert_eq!(
            g.for_plugin("auth"),
            [Capability::HttpClient].into_iter().collect()
        );
        assert!(g.for_plugin("missing").is_empty());
    }

    #[test]
    fn grant_table_unknown_caps_dropped_silently() {
        let g = GrantTable::parse("p=http-client,nonsense");
        let caps = g.for_plugin("p");
        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&Capability::HttpClient));
    }

    #[test]
    fn grant_table_malformed_entries_skipped() {
        let g = GrantTable::parse(";=;noequalshere;p=http-client");
        assert!(g.for_plugin("p").contains(&Capability::HttpClient));
    }

    #[tokio::test]
    async fn empty_host_returns_keep() {
        let host = PluginHost::new().expect("engine");
        let event = FileCreated {
            path: std::path::PathBuf::from("/tmp/nonexistent.mp4"),
            session_id: "sid".into(),
            kind: ArtifactKind::Video,
            browser: None,
            browser_version: None,
            s3_key_pattern: None,
        };
        let verdict = host.artifact_verdict(&event).await;
        assert!(matches!(verdict, ArtifactVerdict::Keep));
    }
}
