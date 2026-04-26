//! Real-time artifact streaming (P6.2).
//!
//! Replaces the finalize-on-end video pipeline with a CDP
//! `Page.startScreencast` consumer that pipes JPEG frames to ffmpeg
//! (H.264 encode) and on to an S3 multipart upload as the session
//! runs. Frames are available seconds after session-end instead of
//! minutes.
//!
//! Phase-6 scope:
//! - this file: traits, config, the per-session streamer wiring
//! - the CDP client + ffmpeg sub-process + S3 multipart coordinator
//!   are still to be filled in (tracked under 0XT-82); the trait
//!   surface defined here is the seam they plug into
//!
//! The driving design constraint: streaming and finalize-on-end MUST
//! coexist. Operators flip per-session via `caps.enableVideo` (off /
//! finalize) plus a `caps.streamVideo: true` flag (stream live).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub fps: u32,
    pub jpeg_quality: u8,
    pub h264_preset: String,
    pub destination: String,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            fps: 15,
            jpeg_quality: 85,
            h264_preset: "veryfast".into(),
            destination: String::new(),
        }
    }
}

#[async_trait]
pub trait VideoStreamer: Send + Sync {
    /// Start streaming for the named session against the upstream
    /// CDP endpoint. Returns when the streamer is fully attached and
    /// the first multipart-upload part is open.
    async fn start(
        &self,
        session_id: &str,
        cdp_url: &str,
        cfg: &StreamConfig,
    ) -> anyhow::Result<()>;

    /// Stop streaming, finalize the multipart upload, return the
    /// destination URI of the final object.
    async fn stop(&self, session_id: &str) -> anyhow::Result<String>;
}
