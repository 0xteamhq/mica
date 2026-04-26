use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Selenium / WebDriver capabilities, parsed from either the W3C
/// `capabilities.alwaysMatch` block or the legacy `desiredCapabilities`
/// block. Selenoid-specific options under `selenoid:options` are merged
/// into the top level so callers see one flat struct.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Caps {
    pub browser_name: Option<String>,
    pub browser_version: Option<String>,
    pub platform: Option<String>,
    #[serde(rename = "enableVNC")]
    pub enable_vnc: bool,
    pub enable_video: bool,
    pub enable_log: bool,
    pub screen_resolution: Option<String>,
    pub video_name: Option<String>,
    pub log_name: Option<String>,
    pub time_zone: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum CapsError {
    #[error("missing capabilities")]
    Missing,
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Caps {
    /// Parse a WebDriver new-session request body. W3C
    /// `capabilities.alwaysMatch` is preferred; `desiredCapabilities`
    /// is the legacy fallback. The legacy `version` key is mapped to
    /// `browserVersion`. `selenoid:options` keys are merged onto the
    /// base capabilities object before deserialization.
    pub fn parse(body: &Value) -> Result<Self, CapsError> {
        let raw = body
            .get("capabilities")
            .and_then(|c| c.get("alwaysMatch"))
            .or_else(|| body.get("desiredCapabilities"))
            .ok_or(CapsError::Missing)?;

        let mut merged = raw.clone();
        if let Some(ext) = raw.get("selenoid:options").cloned()
            && let (Value::Object(base), Value::Object(extra)) = (&mut merged, &ext)
        {
            for (k, v) in extra {
                base.insert(k.clone(), v.clone());
            }
        }

        // Legacy: "version" -> "browserVersion".
        if let Value::Object(map) = &mut merged
            && !map.contains_key("browserVersion")
            && let Some(v) = map.remove("version")
        {
            map.insert("browserVersion".into(), v);
        }

        Ok(serde_json::from_value(merged)?)
    }
}
