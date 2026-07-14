//! /admin/api/state — dashboard snapshot shape + auth gating.

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use mica::backend::HostPorts;
use mica::session::Session;
use tower::ServiceExt;

async fn get_state(app: axum::Router) -> serde_json::Value {
    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn empty_grid_snapshot() {
    let (state, _backend) = common::build_state("http://noop", common::args());
    let json = get_state(common::build_app(state, None)).await;

    assert_eq!(json["capacity"]["total"], 5);
    assert_eq!(json["capacity"]["used"], 0);
    assert_eq!(json["draining"], false);
    assert_eq!(json["sessions"].as_array().unwrap().len(), 0);
    // Fixture registry surfaces name -> {default, versions}.
    let browsers = json["browsers"].as_object().unwrap();
    assert!(!browsers.is_empty());
    for (_, info) in browsers {
        assert!(info["default"].is_string());
        assert!(info["versions"].is_array());
    }
}

#[tokio::test]
async fn session_flags_reflect_host_ports() {
    let (state, _backend) = common::build_state("http://noop", common::args());
    let session = Session::new_full(
        "sid-vnc",
        "http://127.0.0.1:9515".into(),
        HostPorts {
            vnc: Some("5900".into()),
            devtools: None,
            fileserver: None,
            clipboard: None,
        },
        "chrome",
        "126.0",
        Some("alice".into()),
        std::time::Duration::from_secs(60),
        Box::new(|| {}),
        Box::new(|| {}),
    );
    state.sessions.put(session).await;

    let json = get_state(common::build_app(state, None)).await;
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s["id"], "sid-vnc");
    assert_eq!(s["browser"], "chrome");
    assert_eq!(s["version"], "126.0");
    assert_eq!(s["vnc"], true);
    assert_eq!(s["devtools"], false);
    assert_eq!(s["logs"], false);
    assert_eq!(s["owner"], "alice");
    assert!(s["started"].is_string());
}

#[tokio::test]
async fn admin_api_is_auth_gated() {
    let hash = bcrypt::hash("s3cret", 4).unwrap();
    let f = tempfile::NamedTempFile::new().unwrap();
    // Admin row: /admin/api/state exposes session owners, so it now
    // requires the admin role (see the RequireAdmin gate).
    std::fs::write(f.path(), format!("alice:{hash}:admin\n")).unwrap();

    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = common::build_app(state, Some(f.path().to_str().unwrap()));

    // Both the API and the SPA shell require credentials.
    for uri in ["/admin/api/state", "/admin"] {
        let res = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{uri} must be gated"
        );
    }

    let creds = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(b"alice:s3cret")
    );
    let res = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/state")
                .header(axum::http::header::AUTHORIZATION, creds)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
