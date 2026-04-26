use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Selenium / WebDriver capabilities, parsed from either the W3C
/// `capabilities.alwaysMatch` block or the legacy `desiredCapabilities`
/// block. Mica-specific options under `mica:options` are merged into
/// the top level so callers see one flat struct.
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
    /// `mica:options.sessionTimeout` — overrides the server's
    /// default idle timeout for this session, capped at
    /// `--max-timeout`.
    pub session_timeout: Option<String>,
    /// Selenoid: forwarded as `VIDEO_FRAME_RATE` env to the recorder.
    pub video_frame_rate: Option<u32>,
    /// Selenoid: forwarded as `CODEC` env to the recorder.
    pub video_codec: Option<String>,
    /// Selenoid: forwarded as `VIDEO_SIZE`. When unset, falls back to
    /// `screen_resolution`.
    pub video_screen_size: Option<String>,
    /// Selenoid: per-session S3 key template overriding `--s3-prefix`.
    /// Tokens: `$fileName`, `$fileExtension`, `$browserName`,
    /// `$browserVersion`, `$sessionId`, `$fileType`, `$date`.
    pub s3_key_pattern: Option<String>,
    /// Selenoid: docker container hostname.
    pub container_hostname: Option<String>,
    /// Selenoid: extra `/etc/hosts` entries, e.g. `["foo:1.2.3.4"]`.
    /// Merged with `Browser.hosts` from browsers.json.
    #[serde(default)]
    pub hosts_entries: Vec<String>,
    /// Selenoid: per-session DNS servers; maps to `HostConfig.Dns`.
    #[serde(default)]
    pub dns_servers: Vec<String>,
    /// Selenoid: container names mica should `--link` into the session.
    #[serde(default)]
    pub application_containers: Vec<String>,
    /// Selenoid: additional Docker networks to attach the session
    /// container to (post-create).
    #[serde(default)]
    pub additional_networks: Vec<String>,
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
    /// `browserVersion`. `mica:options` keys are merged onto the
    /// base capabilities object before deserialization.
    pub fn parse(body: &Value) -> Result<Self, CapsError> {
        let raw = body
            .get("capabilities")
            .and_then(|c| c.get("alwaysMatch"))
            .or_else(|| body.get("desiredCapabilities"))
            .ok_or(CapsError::Missing)?;

        let mut merged = raw.clone();
        if let Some(ext) = raw.get("mica:options").cloned()
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
