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

fn args() -> Args {
    Args {
        listen: ":4444".into(),
        conf: "tests/fixtures/browsers.json".into(),
        limit: 7,
        timeout: Duration::from_secs(60),
        max_timeout: Duration::from_secs(3600),
        service_startup_timeout: Duration::from_secs(30),
        session_attempt_timeout: Duration::from_secs(30),
        session_delete_timeout: Duration::from_secs(30),
        retry_count: 1,
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
        users: String::new(),
    }
}

#[tokio::test]
async fn status_reports_capacity_and_browsers() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-S", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;

    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new(upstream.uri()));
    let state = AppState::new(cfg, args(), backend);
    let app = handlers::router(state.clone());

    // Empty grid
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 7);
    assert_eq!(json["used"], 0);
    assert_eq!(json["sessions"].as_array().unwrap().len(), 0);
    assert!(
        json["browsers"]["firefox"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("126.0"))
    );
    assert!(
        json["browsers"]["chrome"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("125.0"))
    );

    // Create one session, status reflects it
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wd/hub/session")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["used"], 1);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["browser"], "firefox");
    assert_eq!(sessions[0]["version"], "126.0");
}

#[tokio::test]
async fn ping_v2_includes_session_count() {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new("http://noop"));
    let state = AppState::new(cfg, args(), backend);
    let app = handlers::router(state);

    let res = app
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessions"], 0);
    assert_eq!(json["queue"]["total"], 7);
    assert_eq!(json["queue"]["used"], 0);
}
