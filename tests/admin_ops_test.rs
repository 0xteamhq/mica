//! M2 admin operations: kill session, drain, config reload.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use mica::events::AdminEvent;
use std::time::Duration;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

async fn create_session(app: &axum::Router) -> StatusCode {
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
        .unwrap()
        .status()
}

#[tokio::test]
async fn kill_session_tears_down_and_404s_when_unknown() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-kill", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session/sid-kill"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": null})))
        .mount(&upstream)
        .await;

    let (state, _backend) = common::build_state(&upstream.uri(), common::args());
    let mut rx = state.events.subscribe_admin();
    let app = mica::handlers::router(state.clone());

    assert_eq!(create_session(&app).await, StatusCode::OK);
    let _created = rx.recv().await.unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/admin/api/sessions/sid-kill")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(state.sessions.len(), 0, "session removed from the map");

    // Cancel hook ran: SessionStopped comes through the broadcast.
    let stopped = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timely stop event")
        .unwrap();
    assert!(
        matches!(stopped, AdminEvent::SessionStopped { session_id } if session_id == "sid-kill")
    );

    // Idempotence: the session is gone, so a second kill is a 404.
    let res = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/admin/api/sessions/sid-kill")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn drain_flips_readyz_and_rejects_creates() {
    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = mica::handlers::router(state.clone());

    let drain = |active: bool| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/drain")
                    .header("content-type", "application/json")
                    .body(Body::from(format!("{{\"active\":{active}}}")))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let res = drain(true).await;
    assert_eq!(res.status(), StatusCode::OK);

    let readyz = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::SERVICE_UNAVAILABLE);

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(status.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["draining"], true);

    // New sessions rejected while draining (before touching the queue).
    let res = create_session(&app).await;
    assert_eq!(res, StatusCode::INTERNAL_SERVER_ERROR); // W3C session not created
    assert_eq!(state.queue.used(), 0);
    assert_eq!(state.queue.pending(), 0);

    // Manual drain is reversible.
    drain(false).await;
    let readyz = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readyz.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_reload_picks_up_changes_and_rejects_bad_json() {
    // Own conf file so the test can rewrite it.
    let conf = tempfile::NamedTempFile::new().unwrap();
    std::fs::copy("tests/fixtures/browsers.json", conf.path()).unwrap();
    let mut args = common::args();
    args.conf = conf.path().to_str().unwrap().to_string();
    let (state, _backend) = common::build_state("http://noop", args);
    let app = mica::handlers::router(state.clone());

    let reload = || {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/config/reload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    // Shrink the registry to one browser and reload.
    std::fs::write(
        conf.path(),
        r#"{"chrome": {"default": "1.0", "versions": {"1.0": {"image": "img:1"}}}}"#,
    )
    .unwrap();
    let res = reload().await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["browsers"], 1);
    assert!(state.config().find("chrome", None).is_some());
    assert!(state.config().find("firefox", None).is_none());

    // Bad JSON → 400 and the previous config stays live.
    std::fs::write(conf.path(), "{ not json").unwrap();
    let res = reload().await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(state.config().find("chrome", None).is_some());
}
