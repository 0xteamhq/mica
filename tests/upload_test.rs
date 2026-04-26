use async_trait::async_trait;
use mica::events::{ArtifactKind, EventBus, FileCreated};
use mica::upload::{UploadListener, Uploader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingUploader {
    count: Arc<AtomicUsize>,
    last_session: tokio::sync::Mutex<Option<String>>,
}

#[async_trait]
impl Uploader for RecordingUploader {
    async fn upload(&self, _path: &Path, session_id: &str) -> anyhow::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.last_session.lock().await = Some(session_id.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn upload_listener_runs_on_file_created() {
    let bus = EventBus::new();
    let count = Arc::new(AtomicUsize::new(0));
    let uploader = Arc::new(RecordingUploader {
        count: count.clone(),
        last_session: tokio::sync::Mutex::new(None),
    });
    let listener = Arc::new(UploadListener::new(uploader.clone()));
    bus.add_file_listener(listener).await;

    bus.emit_file(FileCreated {
        path: PathBuf::from("video/sid-A.mp4"),
        session_id: "sid-A".into(),
        kind: ArtifactKind::Video,
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(uploader.last_session.lock().await.as_deref(), Some("sid-A"));
}
