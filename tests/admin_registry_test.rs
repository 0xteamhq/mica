//! M3 registry editing: raw-bytes GET/PUT with validation + atomic
//! persistence.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn setup() -> (tempfile::NamedTempFile, mica::state::AppState, axum::Router) {
    let conf = tempfile::NamedTempFile::new().unwrap();
    std::fs::copy("tests/fixtures/browsers.json", conf.path()).unwrap();
    let mut args = common::args();
    args.conf = conf.path().to_str().unwrap().to_string();
    let (state, _backend) = common::build_state("http://noop", args);
    let app = mica::handlers::router(state.clone());
    (conf, state, app)
}

async fn get_browsers(app: &axum::Router) -> (StatusCode, Vec<u8>) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/api/config/browsers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    (status, body.to_vec())
}

async fn put_browsers(app: &axum::Router, body: &str) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/api/config/browsers")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, json)
}

#[tokio::test]
async fn get_returns_file_verbatim() {
    let (conf, _state, app) = setup();
    let (status, body) = get_browsers(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, std::fs::read(conf.path()).unwrap());
}

#[tokio::test]
async fn put_persists_bytes_and_swaps_config() {
    let (conf, state, app) = setup();
    // Unknown field (`comment`) + non-canonical spacing: both must
    // survive because PUT persists the client's bytes verbatim.
    let body = r#"{
  "comment-preserving-browser": {"default": "9.0", "versions": {"9.0": {"image": "img:9", "futureField": true}}}
}"#;
    let (status, _) = put_browsers(&app, body).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        std::fs::read(conf.path()).unwrap(),
        body.as_bytes(),
        "file is byte-identical to the PUT body"
    );
    assert!(
        state
            .config()
            .find("comment-preserving-browser", None)
            .is_some(),
        "hot-swapped config is live"
    );
    let (_, got) = get_browsers(&app).await;
    assert_eq!(got, body.as_bytes());
}

#[tokio::test]
async fn put_rejects_bad_payloads_and_keeps_file() {
    let (conf, state, app) = setup();
    let before = std::fs::read(conf.path()).unwrap();

    // Malformed JSON.
    let (status, err) = put_browsers(&app, "{ nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("json"));

    // Semantic error: default version not present.
    let (status, err) = put_browsers(
        &app,
        r#"{"chrome": {"default": "99.0", "versions": {"1.0": {"image": "img:1"}}}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("default"));

    assert_eq!(
        std::fs::read(conf.path()).unwrap(),
        before,
        "rejected PUTs leave the file untouched"
    );
    assert!(state.config().find("firefox", None).is_some());
}
