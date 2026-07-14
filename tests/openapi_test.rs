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

fn args() -> Args {
    Args::parse_from(["mica", "--conf", "tests/fixtures/browsers.json"])
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
