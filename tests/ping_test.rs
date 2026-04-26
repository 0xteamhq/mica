use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mica::handlers::ping::router as ping_router;
use tower::ServiceExt;

#[tokio::test]
async fn ping_returns_uptime_and_version() {
    let app = ping_router();
    let response = app
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("version").is_some(), "expected version field");
    assert!(json.get("uptime").is_some(), "expected uptime field");
    assert!(
        json.get("lastReloadTime").is_some(),
        "expected lastReloadTime"
    );
}
