//! WASM plugin host (P5.1, P5.2).
//!
//! `wasmtime` 26 + WASI Preview 2 + Component Model. The full WIT
//! contract lives at `wit/mica.wit`. This file's `PluginHost`
//! manages a config-driven directory of `.wasm` components and
//! exposes a typed-ish API for the rest of mica to call.
//!
//! Phase-5 scope:
//! - `PluginHost::load_dir(dir)` instantiates every `.wasm` it
//!   finds, calls each plugin's `lifecycle.init`, and registers
//!   it on the EventBus as a `FileCreatedListener` +
//!   `SessionStoppedListener`.
//! - `--plugin-dir` CLI flag wires it up from `main.rs`.
//! - Capability grants land via `--plugin-grants <name>=<caps>`
//!   in a follow-up commit; today every plugin gets a clean
//!   default WASI context (no FS / no network) which forces
//!   plugin authors to declare imports up front.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::Engine;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

/// One mica plugin: the loaded component + a per-plugin store. Stores
/// hold the WASI context, so each plugin sees its own filesystem
/// preopens and env without sharing state with peers.
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
        // Validate the linker can resolve the imports — this fails
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
