//! Artifact upload — `Uploader` trait and built-in `S3Uploader`.
//!
//! Wired into the M10 `EventBus` from `main.rs` when `--s3-bucket`
//! is set. Phase 5 WASM plugins implement the same trait via the
//! plugin host, so users can ship custom uploaders (GCS / Azure /
//! self-hosted) without forking mica.

pub mod s3;

use crate::events::{FileCreated, FileCreatedListener};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

#[async_trait]
pub trait Uploader: Send + Sync {
    async fn upload(&self, path: &Path, session_id: &str) -> anyhow::Result<()>;
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
        if let Err(err) = self.uploader.upload(&e.path, &e.session_id).await {
            tracing::warn!(error = %err, path = %e.path.display(), "upload failed");
        } else {
            tracing::info!(path = %e.path.display(), "upload ok");
        }
    }
}
