//! Event bus — async fan-out for `FileCreated` and `SessionStopped`.
//!
//! T42-T44 — listeners register at startup; emitters are called from
//! teardown paths (M8 cancel hooks). Each emit fans out one
//! `tokio::spawn` per listener so a slow uploader can't stall mica.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct FileCreated {
    pub path: PathBuf,
    pub session_id: String,
    pub kind: ArtifactKind,
    /// Per-session metadata that S3-key templates
    /// interpolate. Optional so synthetic `FileCreated` events
    /// (tests, plugins) can leave them unset.
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    /// `caps.s3KeyPattern` — if set, overrides the global
    /// `--s3-prefix` template for this session's artifacts.
    pub s3_key_pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Video,
    Log,
}

#[derive(Debug, Clone)]
pub struct SessionStopped {
    pub session_id: String,
    pub started: std::time::SystemTime,
    pub finished: std::time::SystemTime,
    pub browser: Option<String>,
    pub browser_version: Option<String>,
}

#[async_trait]
pub trait FileCreatedListener: Send + Sync {
    async fn on_file_created(&self, e: &FileCreated);
}

#[async_trait]
pub trait SessionStoppedListener: Send + Sync {
    async fn on_session_stopped(&self, e: &SessionStopped);
}

#[derive(Default, Clone)]
pub struct EventBus {
    file_listeners: Arc<RwLock<Vec<Arc<dyn FileCreatedListener>>>>,
    session_listeners: Arc<RwLock<Vec<Arc<dyn SessionStoppedListener>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add_file_listener(&self, l: Arc<dyn FileCreatedListener>) {
        self.file_listeners.write().await.push(l);
    }

    pub async fn add_session_listener(&self, l: Arc<dyn SessionStoppedListener>) {
        self.session_listeners.write().await.push(l);
    }

    pub async fn emit_file(&self, e: FileCreated) {
        let listeners = self.file_listeners.read().await.clone();
        for l in listeners {
            let l = l.clone();
            let e = e.clone();
            tokio::spawn(async move { l.on_file_created(&e).await });
        }
    }

    pub async fn emit_session(&self, e: SessionStopped) {
        let listeners = self.session_listeners.read().await.clone();
        for l in listeners {
            let l = l.clone();
            let e = e.clone();
            tokio::spawn(async move { l.on_session_stopped(&e).await });
        }
    }
}
