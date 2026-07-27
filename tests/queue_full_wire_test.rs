//! Wire contract for a rejected create when the queue is full.
//!
//! `--disable-queue` and the `X-Mica-No-Wait: 1` header both bypass the
//! blocking `queue.acquire()` and fail fast. The exact shape they fail
//! with is a documented, client-facing contract (README, `/openapi.yaml`,
//! and the docs site all tell clients to match on it): HTTP 500 with the
//! W3C body `{"value":{"error":"session not created",
//! "message":"queue is full"}}`.
//!
//! These tests pin status, `error`, and `message` together so that a
//! change to `WdError`'s `IntoResponse` mapping or to the message string
//! cannot silently break clients written against the published docs.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

/// Upstream that accepts creates, so the only thing that can reject a
/// request in these tests is mica's own queue.
async fn mock_upstream() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-1", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session/sid-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": null})))
        .mount(&upstream)
        .await;
    upstream
}

async fn create(app: &axum::Router, no_wait: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder()
        .method("POST")
        .uri("/wd/hub/session")
        .header("content-type", "application/json");
    if let Some(v) = no_wait {
        b = b.header("X-Mica-No-Wait", v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(FF_BODY)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

/// The published contract, asserted as a whole: status + error + message.
fn assert_queue_full_wire(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "queue-full is documented as HTTP 500 (not 429/503); body: {body}"
    );
    assert_eq!(
        body["value"]["error"], "session not created",
        "W3C error code is the documented match key; body: {body}"
    );
    assert_eq!(
        body["value"]["message"], "queue is full",
        "message is part of the documented contract; body: {body}"
    );
}

/// `--disable-queue`: with the single slot already taken, the next
/// create must fail fast rather than block.
#[tokio::test]
async fn disable_queue_rejects_with_documented_wire_shape() {
    let upstream = mock_upstream().await;
    let mut args = common::args();
    args.limit = 1;
    args.disable_queue = true;
    let (state, _backend) = common::build_state(&upstream.uri(), args);
    let app = common::build_app(state.clone(), None);

    // Occupy the only slot.
    let permit = state.queue.try_acquire().expect("limit is 1, slot is free");

    let (status, body) = create(&app, None).await;
    assert_queue_full_wire(status, &body);

    drop(permit);
}

/// `X-Mica-No-Wait` opts one request out of queueing, with the same wire
/// shape, while the server flag stays off. Every spelling the OpenAPI
/// spec advertises (`"1"`, `"true"`) plus the case-insensitive form the
/// handler accepts is covered, so narrowing the parse would fail here.
#[tokio::test]
async fn no_wait_header_rejects_with_documented_wire_shape() {
    for header in ["1", "true", "TRUE", "True"] {
        let upstream = mock_upstream().await;
        let mut args = common::args();
        args.limit = 1;
        let (state, _backend) = common::build_state(&upstream.uri(), args);
        let app = common::build_app(state.clone(), None);

        let permit = state.queue.try_acquire().expect("limit is 1, slot is free");

        // Bounded: if the header stops being honored, the create falls
        // back to blocking on the queue we are holding. Without this the
        // regression would hang the suite instead of failing it.
        let (status, body) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            create(&app, Some(header)),
        )
        .await
        .unwrap_or_else(|_| {
            panic!("X-Mica-No-Wait: {header} was not honored — create blocked on the queue")
        });
        assert_queue_full_wire(status, &body);

        drop(permit);
    }
}

/// A value outside the accepted set must NOT opt out — the request keeps
/// the default blocking behavior, so it is still queued rather than
/// rejected. Guards against the parse widening to "any value present".
#[tokio::test]
async fn unrecognized_no_wait_value_still_queues() {
    let upstream = mock_upstream().await;
    let mut args = common::args();
    args.limit = 1;
    let (state, _backend) = common::build_state(&upstream.uri(), args);
    let app = common::build_app(state.clone(), None);

    let permit = state.queue.try_acquire().expect("limit is 1, slot is free");

    // "0" is not an opt-out, so this create must block on the queue
    // rather than return queue-is-full.
    let pending = tokio::spawn({
        let app = app.clone();
        async move { create(&app, Some("0")).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !pending.is_finished(),
        "unrecognized value must queue, not fail fast"
    );

    drop(permit);
    let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
        .await
        .expect("create must complete once a slot frees")
        .unwrap();
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

/// The escape hatches must not fire while capacity remains — otherwise
/// the tests above would pass for the wrong reason.
#[tokio::test]
async fn no_wait_succeeds_when_a_slot_is_free() {
    let upstream = mock_upstream().await;
    let mut args = common::args();
    args.limit = 1;
    let (state, _backend) = common::build_state(&upstream.uri(), args);
    let app = common::build_app(state, None);

    let (status, body) = create(&app, Some("1")).await;
    assert_eq!(status, StatusCode::OK, "slot was free; body: {body}");
    assert_eq!(body["value"]["sessionId"], "sid-1");
}
