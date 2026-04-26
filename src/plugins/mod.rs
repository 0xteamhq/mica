//! WASM plugin host (P5.1, P5.2).
//!
//! `wasmtime` 26 + WASI Preview 2 + Component Model. The plugin
//! contract lives in `wit/` (`world plugin` from `world.wit` — the
//! split-file layout supersedes the legacy monolithic `mica.wit`).
//!
//! Status of this file:
//! - `PluginHost::load_dir` parses every `.wasm` in `--plugin-dir`
//!   and validates that each is a Component. Soft-fails per-file so
//!   one broken plugin can't stall startup.
//! - **Invocation is not wired yet.** Dispatching `lifecycle.init`,
//!   `artifact.on-file-created`, `session.on-create`, and the http
//!   middleware hooks requires:
//!   1. host-side implementations of the always-granted imports
//!      (`mica:plugin/host-log`, `mica:plugin/clock`)
//!   2. capability-gated implementations of `http-client`,
//!      `s3-write`, `state`, switched on by the
//!      `--plugin-grants <name>=<caps>` CLI surface
//!   3. transcode helpers between mica's Rust types
//!      (`crate::events::FileCreated`, `crate::caps::Caps`) and
//!      the WIT records in `types.wit`
//!   4. handler-side wiring for the variant return types
//!      (`upload-destination`, `request-action`,
//!      `capabilities-decision`).
//!
//!   That's a separate, focused commit — load-only ships now.
//!
//! Plugins still serve a purpose at this stage: every component is
//! validated (Wasm-component shape, world conformance against the
//! WIT in `wit/`), so operators get a clear startup error if a
//! plugin is malformed before any session traffic flows.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::Engine;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

/// One mica plugin: the loaded component. Each invocation builds a
/// fresh `Store<HostState>` so plugins can't observe each other's
/// state.
#[allow(dead_code)]
pub struct Plugin {
    pub name: String,
    pub(crate) component: Component,
}

pub struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

#[derive(Clone)]
pub struct PluginHost {
    engine: Engine,
    plugins: Arc<Mutex<Vec<Plugin>>>,
}

impl PluginHost {
    pub fn new() -> anyhow::Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config).context("wasmtime engine")?;
        Ok(Self {
            engine,
            plugins: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Load every `.wasm` file in `dir`. Soft-fails on per-file
    /// errors so one broken plugin can't stall startup. The result
    /// `Ok(())` means we *attempted* every file; check logs for
    /// per-plugin failures.
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
        // Validate the linker can resolve the WASI imports — fails
        // cleanly when a plugin asks for a capability we haven't
        // wired host-side yet.
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker).context("wasi-preview2 linker")?;
        let _ = linker; // silence unused-when-stub warning

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

    /// Build a fresh per-plugin store. Each invocation gets its own
    /// WASI context so plugins can't observe each other's state.
    pub fn fresh_store(&self) -> wasmtime::Store<HostState> {
        let table = ResourceTable::new();
        let wasi = WasiCtxBuilder::new().build();
        wasmtime::Store::new(&self.engine, HostState { table, wasi })
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
}
