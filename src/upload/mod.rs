//! Artifact upload — `Uploader` trait and built-in `S3Uploader`.
//!
//! Wired into the M10 `EventBus` from `main.rs` when `--s3-bucket`
//! is set. Phase 5 WASM plugins implement the same trait via the
//! plugin host, so users can ship custom uploaders (GCS / Azure /
//! self-hosted) without forking mica.

pub mod s3;

use crate::events::{ArtifactKind, FileCreated, FileCreatedListener};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Uploader: Send + Sync {
    async fn upload(&self, e: &FileCreated) -> anyhow::Result<()>;
}

/// Adapter that turns any `Uploader` into a `FileCreatedListener`.
pub struct UploadListener {
    uploader: Arc<dyn Uploader>,
}

impl UploadListener {
    pub fn new(uploader: Arc<dyn Uploader>) -> Self {
        Self { uploader }
    }
}

#[async_trait]
impl FileCreatedListener for UploadListener {
    async fn on_file_created(&self, e: &FileCreated) {
        if let Err(err) = self.uploader.upload(e).await {
            tracing::warn!(error = %err, path = %e.path.display(), "upload failed");
        } else {
            tracing::info!(path = %e.path.display(), "upload ok");
        }
    }
}

/// Selenoid-compatible token substitution for S3 keys (Selenoid
/// upload/s3.go:139-157). Recognized tokens:
///
///  - `$fileName`         — full file name (`abc.mp4`)
///  - `$fileNameWithoutExt` — without extension (`abc`)
///  - `$fileExtension`    — extension without dot (`mp4`)
///  - `$browserName` / `$browserVersion`
///  - `$sessionId`
///  - `$fileType`         — `video` | `log`
///  - `$date`             — `YYYY-MM-DD`
pub fn render_template(tpl: &str, e: &FileCreated) -> String {
    let file_name = e
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&e.session_id)
        .to_string();
    let file_ext = e
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let stem = e
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&e.session_id)
        .to_string();
    let file_type = match e.kind {
        ArtifactKind::Video => "video",
        ArtifactKind::Log => "log",
    };
    let date = chrono_today();
    let mut out = tpl.to_string();
    for (token, value) in [
        ("$fileNameWithoutExt", stem.as_str()),
        ("$fileName", file_name.as_str()),
        ("$fileExtension", file_ext.as_str()),
        ("$browserName", e.browser.as_deref().unwrap_or("")),
        (
            "$browserVersion",
            e.browser_version.as_deref().unwrap_or(""),
        ),
        ("$sessionId", e.session_id.as_str()),
        ("$fileType", file_type),
        ("$date", date.as_str()),
    ] {
        out = out.replace(token, value);
    }
    out
}

fn chrono_today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Pure-stdlib date formatting — civil date from epoch seconds.
    // Avoids dragging in chrono just for "YYYY-MM-DD".
    let days = (secs / 86_400) as i64;
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_to_ymd(mut days: i64) -> (i32, u32, u32) {
    // Algorithm: Howard Hinnant, civil-from-days
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ArtifactKind;
    use std::path::PathBuf;

    fn ev() -> FileCreated {
        FileCreated {
            path: PathBuf::from("video/sid-A.mp4"),
            session_id: "sid-A".into(),
            kind: ArtifactKind::Video,
            browser: Some("chrome".into()),
            browser_version: Some("126.0".into()),
            s3_key_pattern: None,
        }
    }

    #[test]
    fn template_substitutes_known_tokens() {
        let s = render_template(
            "$browserName/$browserVersion/$sessionId/$fileNameWithoutExt.$fileExtension",
            &ev(),
        );
        assert_eq!(s, "chrome/126.0/sid-A/sid-A.mp4");
    }

    #[test]
    fn template_handles_filename_token() {
        let s = render_template("$fileType-$fileName", &ev());
        assert_eq!(s, "video-sid-A.mp4");
    }

    #[test]
    fn template_unknown_tokens_pass_through() {
        let s = render_template("foo/$unknownToken/bar", &ev());
        assert_eq!(s, "foo/$unknownToken/bar");
    }

    #[test]
    fn date_token_yields_yyyy_mm_dd() {
        let s = render_template("$date", &ev());
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 4);
    }
}
