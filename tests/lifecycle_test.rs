//! M8 lifecycle tests against `MockBackend` + `wiremock` for the
//! upstream WebDriver. Covers T29 (create), T30 (proxy), T31 (delete),
//! T32 (idle teardown), T34 (retry).

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use mica::backend::mock::MockBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

fn args_with(timeout_ms: u64, retry_count: u32) -> Args {
    Args {
        listen: ":4444".into(),
        conf: "tests/fixtures/browsers.json".into(),
        limit: 5,
        timeout: Duration::from_millis(timeout_ms),
        max_timeout: Duration::from_secs(3600),
        service_startup_timeout: Duration::from_secs(30),
        session_attempt_timeout: Duration::from_secs(30),
        session_delete_timeout: Duration::from_secs(30),
        retry_count,
        video_output_dir: "video".into(),
        log_output_dir: "logs".into(),
        container_network: "default".into(),
        cpu: String::new(),
        memory: String::new(),
        enable_file_upload: false,
        disable_queue: false,
        graceful_period: Duration::from_secs(300),
        save_all_logs: false,
        disable_privileged: false,
        log_conf: String::new(),
        s3_bucket: String::new(),
        s3_region: String::new(),
        s3_prefix: String::new(),
        warm_pool_min: 0,
        warm_pool_max: 16,
        warm_pool_idle_ttl: Duration::from_secs(300),
        backend: "docker".into(),
        k8s_namespace: "default".into(),
        k8s_runtime_class: String::new(),
        replica_id: String::new(),
        isolation: "auto".into(),
        plugin_dir: String::new(),
    }
}

async fn build_state(upstream_uri: &str, args: Args) -> (AppState, Arc<MockBackend>) {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new(upstream_uri));
    let state = AppState::new(cfg, args, backend.clone());
    (state, backend)
}

#[tokio::test]
async fn create_session_happy_path() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "real-id-123", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;

    let (state, _backend) = build_state(&upstream.uri(), args_with(60_000, 1)).await;
    let app = handlers::router(state.clone());

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(FF_BODY))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["value"]["sessionId"].as_str(),
        Some("real-id-123"),
        "upstream sessionId returned unchanged"
    );
    // Allow async-spawned things (idle watcher) to settle.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(state.queue.used(), 1, "permit promoted to used");
    assert_eq!(state.sessions.len(), 1);
}

#[tokio::test]
async fn proxy_forwards_to_upstream_and_resets_idle() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-A", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("GET"))
        .and(path("/session/sid-A/title"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value":"ok"})))
        .mount(&upstream)
        .await;

    let (state, _backend) = build_state(&upstream.uri(), args_with(60_000, 1)).await;
    let app = handlers::router(state.clone());

    // create
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(FF_BODY))
                .unwrap(),
        )
        .await
        .unwrap();

    // proxy
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/wd/hub/session/sid-A/title")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn delete_tears_session_down() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-D", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session/sid-D"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value":null})))
        .mount(&upstream)
        .await;

    let (state, backend) = build_state(&upstream.uri(), args_with(60_000, 1)).await;
    let app = handlers::router(state.clone());

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(FF_BODY))
                .unwrap(),
        )
        .await
        .unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/wd/hub/session/sid-D")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // The cancel hook spawns the stopper in the background.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(state.sessions.len(), 0, "session removed");
    assert_eq!(
        backend.stop_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "stopper called exactly once"
    );
    assert_eq!(state.queue.used(), 0, "permit released");
}

#[tokio::test]
async fn idle_timeout_tears_session_down() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-I", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session/sid-I"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&upstream)
        .await;

    let (state, backend) = build_state(&upstream.uri(), args_with(100, 1)).await;
    let app = handlers::router(state.clone());

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(FF_BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(state.sessions.len(), 1);

    // Wait past the 100 ms idle timeout.
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert_eq!(state.sessions.len(), 0, "idle reaper removed the session");
    assert_eq!(
        backend.stop_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "stopper called once via cancel hook"
    );
    assert_eq!(state.queue.used(), 0, "permit released");
}

#[tokio::test]
async fn retry_recovers_from_transient_500() {
    let upstream = MockServer::start().await;
    // First call: 500. Second call: 200. Mock library serves them in
    // mount order with `up_to_n_times`.
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-R", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;

    let (state, _backend) = build_state(&upstream.uri(), args_with(60_000, 2)).await;
    let app = handlers::router(state.clone());

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(FF_BODY))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK, "succeeded after one retry");
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["value"]["sessionId"].as_str(), Some("sid-R"));
}
