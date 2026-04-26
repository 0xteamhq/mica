use serde::Deserialize;
use std::collections::HashMap;

/// One browser image entry from `browsers.json`.
///
/// Mirrors Selenoid's `config.Browser` (selenoid/config/config.go:52-66) so
/// existing `browsers.json` files load unchanged. The `image` field is kept
/// as a `serde_json::Value` because Selenoid uses it both as a string
/// (Docker mode) and as an array (driver mode); driver mode is dropped in
/// Phase 1 (see Task T7).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    pub image: serde_json::Value,
    #[serde(default = "default_port")]
    pub port: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub tmpfs: HashMap<String, String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub shm_size: Option<u64>,
    #[serde(default)]
    pub mem: Option<String>,
    #[serde(default)]
    pub cpu: Option<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub sysctl: HashMap<String, String>,
}

fn default_port() -> String {
    "4444".into()
}

impl Browser {
    /// Returns the Docker image name when `image` is a string, otherwise
    /// `None` (driver-mode array entries are unsupported in Phase 1).
    pub fn docker_image(&self) -> Option<&str> {
        self.image.as_str()
    }
}

/// Per-browser version table from `browsers.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Versions {
    pub default: String,
    pub versions: HashMap<String, Browser>,
}
