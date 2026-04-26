use async_trait::async_trait;
use mica::events::{
    ArtifactKind, EventBus, FileCreated, FileCreatedListener, SessionStopped,
    SessionStoppedListener,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

struct CountingFile(Arc<AtomicUsize>);
struct CountingSession(Arc<AtomicUsize>);

#[async_trait]
impl FileCreatedListener for CountingFile {
    async fn on_file_created(&self, _e: &FileCreated) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl SessionStoppedListener for CountingSession {
    async fn on_session_stopped(&self, _e: &SessionStopped) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn emit_fans_out_to_all_listeners() {
    let bus = EventBus::new();
    let file_count = Arc::new(AtomicUsize::new(0));
    let session_count = Arc::new(AtomicUsize::new(0));
    bus.add_file_listener(Arc::new(CountingFile(file_count.clone())))
        .await;
    bus.add_file_listener(Arc::new(CountingFile(file_count.clone())))
        .await;
    bus.add_session_listener(Arc::new(CountingSession(session_count.clone())))
        .await;

    bus.emit_file(FileCreated {
        path: PathBuf::from("video/sid.mp4"),
        session_id: "sid".into(),
        kind: ArtifactKind::Video,
        browser: None,
        browser_version: None,
        s3_key_pattern: None,
    })
    .await;
    bus.emit_session(SessionStopped {
        session_id: "sid".into(),
        started: SystemTime::now(),
        finished: SystemTime::now(),
        browser: Some("firefox".into()),
        browser_version: Some("126.0".into()),
    })
    .await;

    // emits spawn tasks; give them a moment.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(file_count.load(Ordering::SeqCst), 2);
    assert_eq!(session_count.load(Ordering::SeqCst), 1);
}
