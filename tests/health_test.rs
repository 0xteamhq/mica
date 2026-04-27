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

fn args(limit: u32) -> Args {
    Args {
        listen: ":4444".into(),
        conf: "tests/fixtures/browsers.json".into(),
        limit,
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
        plugin_grants: String::new(),
        plugin_state_dir: String::new(),
        plugin_on_create_timeout: Duration::from_secs(5),
        plugin_shutdown_timeout: Duration::from_secs(5),
        plugin_config: String::new(),
    }
}

fn build_state(limit: u32) -> AppState {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new("http://noop"));
    AppState::new(cfg, args(limit), backend)
}

#[tokio::test]
async fn healthz_always_ok() {
    let app = handlers::router(build_state(5));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn readyz_ok_when_queue_has_capacity() {
    let app = handlers::router(build_state(5));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_returns_prometheus_text_when_disabled() {
    // No metrics handle attached → `/metrics` still responds with a
    // useful body (so K8s scrapers don't break) but flags the state.
    let app = handlers::router(build_state(5));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("text/plain"));
    let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.contains("metrics not enabled"));
}
