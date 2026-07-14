use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use clap::Parser;
use mica::backend::mock::MockBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::state::AppState;
use std::sync::Arc;
use tower::ServiceExt;

fn args(limit: u32) -> Args {
    Args::parse_from([
        "mica",
        "--conf",
        "tests/fixtures/browsers.json",
        "--limit",
        &limit.to_string(),
    ])
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
