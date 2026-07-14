//! M3 per-user quotas: fail-fast before the queue, release on
//! teardown, open-mode bypass.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use mica::quota::Quotas;
use std::collections::HashMap;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

fn basic(pair: &str) -> String {
    format!(
        "Basic {}",
        general_purpose::STANDARD.encode(pair.as_bytes())
    )
}

async fn create(app: &axum::Router, auth: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder()
        .method("POST")
        .uri("/wd/hub/session")
        .header("content-type", "application/json");
    if let Some(a) = auth {
        b = b.header(axum::http::header::AUTHORIZATION, a);
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

/// Two mocks: first create returns sid-1 (once), later ones sid-2.
async fn mock_upstream() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-1", "capabilities": {} }
        })))
        .up_to_n_times(1)
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "sid-2", "capabilities": {} }
        })))
        .mount(&upstream)
        .await;
    for sid in ["sid-1", "sid-2"] {
        Mock::given(method("DELETE"))
            .and(path(format!("/session/{sid}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": null})),
            )
            .mount(&upstream)
            .await;
    }
    upstream
}

#[tokio::test]
async fn quota_fails_fast_and_releases_on_delete() {
    let upstream = mock_upstream().await;
    let h = bcrypt::hash("pw", 4).unwrap();
    let users = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(users.path(), format!("alice:{h}\n")).unwrap();

    let (state, _backend) = common::build_state(&upstream.uri(), common::args());
    state.quotas.store(Quotas {
        default: 0,
        users: HashMap::from([("alice".to_string(), 1)]),
    });
    let app = common::build_app(state.clone(), Some(users.path().to_str().unwrap()));
    let creds = basic("alice:pw");

    let (status, _) = create(&app, Some(&creds)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state.queue.used(), 1);

    // Second create exceeds alice's quota of 1: fails fast without
    // touching the queue (used stays 1, nothing queued/pending).
    let (status, body) = create(&app, Some(&creds)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        body["value"]["message"]
            .as_str()
            .unwrap()
            .contains("quota exceeded"),
        "got: {body}"
    );
    assert_eq!(state.queue.used(), 1);
    assert_eq!(state.queue.queued(), 0);
    assert_eq!(state.queue.pending(), 0);

    // Tearing the session down frees the quota unit.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/wd/hub/session/sid-1")
                .header(axum::http::header::AUTHORIZATION, &creds)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let (status, _) = create(&app, Some(&creds)).await;
    assert_eq!(status, StatusCode::OK, "slot freed after delete");
}

#[tokio::test]
async fn open_mode_sessions_bypass_quotas() {
    let upstream = mock_upstream().await;
    let (state, _backend) = common::build_state(&upstream.uri(), common::args());
    // Tight default limit, but with auth disabled there is no owner
    // to meter — both creates pass.
    state.quotas.store(Quotas {
        default: 1,
        users: HashMap::new(),
    });
    let app = common::build_app(state, None);

    let (s1, _) = create(&app, None).await;
    let (s2, _) = create(&app, None).await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
}
