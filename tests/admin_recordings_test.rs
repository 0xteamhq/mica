//! /admin/api/recordings — reconstructs revisitable past sessions from
//! the artifact files left on disk (mica keeps no in-memory history).

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use tower::ServiceExt;

async fn get_recordings(app: axum::Router) -> serde_json::Value {
    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/recordings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Args pointed at the given video/log dirs (open mode = no users file).
fn state_for(video: &str, logs: &str) -> mica::state::AppState {
    let mut args = common::args();
    args.video_output_dir = video.to_string();
    args.log_output_dir = logs.to_string();
    common::build_state("http://noop", args).0
}

fn row<'a>(arr: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    arr.as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no recording {id} in {arr}"))
}

#[tokio::test]
async fn reconstructs_sessions_from_video_and_log_files() {
    let video = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    // sess-a: both video + log; sess-b: video only; sess-c: log only.
    std::fs::write(video.path().join("sess-a.mp4"), b"aaaa").unwrap();
    std::fs::write(logs.path().join("sess-a.log"), b"log-a").unwrap();
    std::fs::write(video.path().join("sess-b.mp4"), b"bbbbbbbb").unwrap();
    std::fs::write(logs.path().join("sess-c.log"), b"log-c").unwrap();
    // A non-artifact file (wrong extension) must be ignored, not listed.
    std::fs::write(video.path().join("notes.txt"), b"ignore").unwrap();
    // A bare `.mp4`/`.log` (extension only) must not produce an empty id.
    std::fs::write(video.path().join(".mp4"), b"x").unwrap();
    std::fs::write(logs.path().join(".log"), b"x").unwrap();

    let state = state_for(
        video.path().to_str().unwrap(),
        logs.path().to_str().unwrap(),
    );
    let json = get_recordings(common::build_app(state, None)).await;

    assert_eq!(
        json.as_array().unwrap().len(),
        3,
        "one row per session id; notes.txt and bare .mp4/.log ignored: {json}"
    );
    assert!(
        json.as_array().unwrap().iter().all(|r| !r["id"].as_str().unwrap().is_empty()),
        "no empty-id rows: {json}"
    );

    let a = row(&json, "sess-a");
    assert!(a["video"].as_bool().unwrap());
    assert!(a["log"].as_bool().unwrap());
    assert_eq!(a["videoBytes"].as_u64().unwrap(), 4);
    assert!(!a["modified"].as_str().unwrap().is_empty());

    let b = row(&json, "sess-b");
    assert!(b["video"].as_bool().unwrap());
    assert!(!b["log"].as_bool().unwrap());
    assert_eq!(b["videoBytes"].as_u64().unwrap(), 8);

    // Log-only session is still revisitable, with no video.
    let c = row(&json, "sess-c");
    assert!(!c["video"].as_bool().unwrap());
    assert!(c["log"].as_bool().unwrap());
    assert_eq!(c["videoBytes"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn empty_when_output_dirs_are_absent() {
    // Fresh node that never recorded anything: missing dirs are treated
    // as "no recordings", not an error.
    let state = state_for("/no/such/mica-video-dir", "/no/such/mica-log-dir");
    let json = get_recordings(common::build_app(state, None)).await;
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn recordings_require_admin_role() {
    let video = tempfile::tempdir().unwrap();
    std::fs::write(video.path().join("sess-x.mp4"), b"x").unwrap();
    let logs = tempfile::tempdir().unwrap();

    // htpasswd with a single NON-admin user.
    let hash = bcrypt::hash("s3cret", 4).unwrap();
    let users = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(users.path(), format!("alice:{hash}\n")).unwrap();

    let state = state_for(
        video.path().to_str().unwrap(),
        logs.path().to_str().unwrap(),
    );
    let app = common::build_app(state, Some(users.path().to_str().unwrap()));

    let creds = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(b"alice:s3cret")
    );
    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/recordings")
                .header(axum::http::header::AUTHORIZATION, creds)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Recordings can expose past-session ids, so the read is admin-only
    // (mirrors /admin/api/state).
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
