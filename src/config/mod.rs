pub mod browser;

use browser::{Browser, Versions};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Parsed `browsers.json`.
#[derive(Debug, Clone, Default)]
pub struct Config {
    inner: HashMap<String, Versions>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let bytes = std::fs::read(path)?;
        let inner: HashMap<String, Versions> = serde_json::from_slice(&bytes)?;
        Ok(Self { inner })
    }

    /// Resolve `(browser_name, requested_version)` to a concrete
    /// `(Browser, version_string)`:
    ///
    /// - `None` or empty `requested_version` → the browser's `default`.
    /// - exact key match → that version.
    /// - prefix match (e.g. `"124"` matches `"124.0"`) when no exact key.
    /// - returns `None` if the browser, the version, or the resolved
    ///   image is unsupported (driver-mode array entries are skipped).
    pub fn find(&self, name: &str, requested_version: Option<&str>) -> Option<(Browser, String)> {
        let versions = self.inner.get(name)?;
        let target = match requested_version {
            None | Some("") => versions.default.clone(),
            Some(v) => {
                if versions.versions.contains_key(v) {
                    v.to_string()
                } else {
                    versions
                        .versions
                        .keys()
                        .find(|k| k.starts_with(v))
                        .cloned()?
                }
            }
        };
        let browser = versions.versions.get(&target).cloned()?;
        browser.docker_image()?;
        Some((browser, target))
    }

    /// Snapshot of available browsers — used by `/status`.
    pub fn snapshot(&self) -> &HashMap<String, Versions> {
        &self.inner
    }
}
