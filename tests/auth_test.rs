use arc_swap::ArcSwap;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use mica::auth::{AuthState, AuthSwap, require_basic_auth};
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
    }
}

fn write_users(user: &str, pass: &str) -> tempfile::NamedTempFile {
    let h = bcrypt::hash(pass, 4).expect("bcrypt");
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), format!("{user}:{h}\n")).unwrap();
    f
}

fn build_app(users_path: Option<&str>) -> axum::Router {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new("http://noop"));
    let state = AppState::new(cfg, args(), backend);
    let auth_state = match users_path {
        Some(p) => AuthState::load(p).unwrap(),
        None => AuthState::empty(),
    };
    let auth: AuthSwap = Arc::new(ArcSwap::from_pointee(auth_state));
    handlers::router(state).layer(axum::middleware::from_fn_with_state(
        auth,
        require_basic_auth,
    ))
}

fn basic(user_pass: &str) -> String {
    format!(
        "Basic {}",
        general_purpose::STANDARD.encode(user_pass.as_bytes())
    )
}

#[tokio::test]
async fn no_users_file_means_open() {
    let app = build_app(None);
    // /status is gated when auth is on; with no auth file it stays open.
    let res = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn open_paths_bypass_auth_even_when_configured() {
    let f = write_users("alice", "s3cret");
    let app = build_app(Some(f.path().to_str().unwrap()));
    for path in ["/ping", "/healthz", "/readyz", "/metrics", "/openapi.yaml"] {
        let res = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path} should be open");
    }
}

#[tokio::test]
async fn gated_path_without_creds_returns_401() {
    let f = write_users("alice", "s3cret");
    let app = build_app(Some(f.path().to_str().unwrap()));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let www = res
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(www.contains("Basic"));
    assert!(www.contains("realm"));
    let body = to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"unauthorized");
}

#[tokio::test]
async fn gated_path_with_valid_creds_passes() {
    let f = write_users("alice", "s3cret");
    let app = build_app(Some(f.path().to_str().unwrap()));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header(axum::http::header::AUTHORIZATION, basic("alice:s3cret"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn gated_path_with_wrong_creds_returns_401() {
    let f = write_users("alice", "s3cret");
    let app = build_app(Some(f.path().to_str().unwrap()));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header(axum::http::header::AUTHORIZATION, basic("alice:wrong"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
