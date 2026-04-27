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

fn args() -> Args {
    Args {
        listen: ":4444".into(),
        conf: "tests/fixtures/browsers.json".into(),
        limit: 5,
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
    }
}

#[tokio::test]
async fn openapi_yaml_serves_spec() {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new("http://noop"));
    let state = AppState::new(cfg, args(), backend);
    let app = handlers::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/openapi.yaml")
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
    assert_eq!(ct, "application/yaml");
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    // Spot-check: the doc should declare itself as OpenAPI 3.0.x and
    // include the WebDriver session create operation.
    assert!(body.starts_with("openapi: 3."));
    assert!(body.contains("title: mica"));
    assert!(body.contains("/wd/hub/session"));
    assert!(body.contains("X-Mica-No-Wait"));
    assert!(body.contains("MicaOptions"));
}
