//! AdminEvent broadcast + /admin/api/events SSE surface.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mica::events::AdminEvent;
use std::time::Duration;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

/// A real create/delete cycle emits SessionCreated then
/// SessionStopped on the admin broadcast channel.
#[tokio::test]
async fn create_delete_cycle_broadcasts_admin_events() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-adm", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session/sid-adm"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": null})))
        .mount(&upstream)
        .await;

    let (state, _backend) = common::build_state(&upstream.uri(), common::args());
    let mut rx = state.events.subscribe_admin();
    let app = mica::handlers::router(state.clone());

    let res = app
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
    assert_eq!(res.status(), StatusCode::OK);

    let created = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timely SessionCreated")
        .expect("channel open");
    match created {
        AdminEvent::SessionCreated {
            session_id,
            browser,
            owner,
            ..
        } => {
            assert_eq!(session_id, "sid-adm");
            assert_eq!(browser, "firefox");
            assert!(owner.is_none(), "owner lands in M2");
        }
        other => panic!("expected SessionCreated, got {other:?}"),
    }

    let res = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/wd/hub/session/sid-adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let stopped = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timely SessionStopped")
        .expect("channel open");
    match stopped {
        AdminEvent::SessionStopped { session_id } => assert_eq!(session_id, "sid-adm"),
        other => panic!("expected SessionStopped, got {other:?}"),
    }
}

/// The SSE endpoint answers with an event-stream and delivers the
/// immediate first `stats` frame.
#[tokio::test]
async fn sse_endpoint_streams_stats() {
    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = mica::handlers::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );

    // Read frames until the first stats event (the interval's first
    // tick is immediate).
    let mut body = res.into_body();
    let mut buf = Vec::new();
    let first_stats = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.unwrap().into_data() {
                buf.extend_from_slice(&data);
                let text = String::from_utf8_lossy(&buf);
                if text.contains("event: stats") && text.contains("\n\n") {
                    return text.to_string();
                }
            }
        }
        panic!("stream ended before stats frame")
    })
    .await
    .expect("timely stats frame");

    assert!(first_stats.contains("\"total\":5"));
    assert!(first_stats.contains("\"draining\":false"));
}
