//! M2 role-based gating of the mutating admin endpoints.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use tower::ServiceExt;

fn users_file() -> tempfile::NamedTempFile {
    let h = bcrypt::hash("s3cret", 4).unwrap();
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), format!("root:{h}:admin\nalice:{h}\n")).unwrap();
    f
}

fn basic(pair: &str) -> String {
    format!(
        "Basic {}",
        general_purpose::STANDARD.encode(pair.as_bytes())
    )
}

fn drain_req(auth: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/admin/api/drain")
        .header("content-type", "application/json");
    if let Some(a) = auth {
        b = b.header(axum::http::header::AUTHORIZATION, a);
    }
    b.body(Body::from(r#"{"active":false}"#)).unwrap()
}

#[tokio::test]
async fn admin_role_gates_mutations() {
    let f = users_file();
    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = common::build_app(state, Some(f.path().to_str().unwrap()));

    // No creds at all → 401 from the Basic gate.
    let res = app.clone().oneshot(drain_req(None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Valid non-admin user → authenticated but 403 on mutation.
    let res = app
        .clone()
        .oneshot(drain_req(Some(&basic("alice:s3cret"))))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Same non-admin user can read the dashboard state.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/api/state")
                .header(axum::http::header::AUTHORIZATION, basic("alice:s3cret"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Admin passes.
    let res = app
        .oneshot(drain_req(Some(&basic("root:s3cret"))))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn open_mode_allows_mutations() {
    // No users file → RequireAdmin mirrors the open posture of the
    // Basic gate.
    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = common::build_app(state, None);
    let res = app.oneshot(drain_req(None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
