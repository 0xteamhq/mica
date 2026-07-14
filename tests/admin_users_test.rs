//! M3 user management: CRUD writes flow through to the live auth gate.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose;
use tower::ServiceExt;

fn basic(pair: &str) -> String {
    format!(
        "Basic {}",
        general_purpose::STANDARD.encode(pair.as_bytes())
    )
}

fn setup() -> (tempfile::NamedTempFile, axum::Router) {
    let h = bcrypt::hash("s3cret", 4).unwrap();
    let users = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(users.path(), format!("root:{h}:admin\n")).unwrap();
    let mut args = common::args();
    args.users = users.path().to_str().unwrap().to_string();
    let (state, _backend) = common::build_state("http://noop", args);
    let app = common::build_app(state, Some(users.path().to_str().unwrap()));
    (users, app)
}

async fn send(app: &axum::Router, req: Request<Body>) -> StatusCode {
    app.clone().oneshot(req).await.unwrap().status()
}

fn put_user(name: &str, body: &str, auth: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(format!("/admin/api/users/{name}"))
        .header("content-type", "application/json")
        .header(axum::http::header::AUTHORIZATION, auth)
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn status_req(auth: &str) -> Request<Body> {
    Request::builder()
        .uri("/status")
        .header(axum::http::header::AUTHORIZATION, auth)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn user_crud_flows_through_to_live_auth() {
    let (users_file, app) = setup();
    let root = basic("root:s3cret");

    // bob can't authenticate yet.
    assert_eq!(
        send(&app, status_req(&basic("bob:pw"))).await,
        StatusCode::UNAUTHORIZED
    );

    // Admin creates bob → the gate accepts him immediately.
    assert_eq!(
        send(&app, put_user("bob", r#"{"password":"pw"}"#, &root)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, status_req(&basic("bob:pw"))).await,
        StatusCode::OK
    );

    // The file was rewritten in v2 format and contains both rows.
    let contents = std::fs::read_to_string(users_file.path()).unwrap();
    assert!(contents.contains("root:") && contents.contains(":admin"));
    assert!(contents.contains("bob:"));
    assert!(!contents.contains("pw"), "plaintext never hits the file");

    // bob is not an admin: mutating endpoints reject him.
    assert_eq!(
        send(
            &app,
            put_user("eve", r#"{"password":"x"}"#, &basic("bob:pw"))
        )
        .await,
        StatusCode::FORBIDDEN
    );

    // Role-only promotion (no password) keeps bob's hash working.
    assert_eq!(
        send(&app, put_user("bob", r#"{"admin":true}"#, &root)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            put_user("eve", r#"{"password":"x"}"#, &basic("bob:pw"))
        )
        .await,
        StatusCode::NO_CONTENT,
        "promoted bob can now mutate"
    );

    // Deleting bob revokes access on the next request.
    let del = Request::builder()
        .method("DELETE")
        .uri("/admin/api/users/bob")
        .header(axum::http::header::AUTHORIZATION, &root)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, del).await, StatusCode::NO_CONTENT);
    assert_eq!(
        send(&app, status_req(&basic("bob:pw"))).await,
        StatusCode::UNAUTHORIZED
    );

    // Guardrails: unknown delete 404s, role-only PUT for a new user
    // 400s, bad names 400.
    let del = Request::builder()
        .method("DELETE")
        .uri("/admin/api/users/ghost")
        .header(axum::http::header::AUTHORIZATION, &root)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, del).await, StatusCode::NOT_FOUND);
    assert_eq!(
        send(&app, put_user("ghost", r#"{"admin":true}"#, &root)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&app, put_user("a:b", r#"{"password":"x"}"#, &root)).await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn users_api_conflicts_when_auth_is_open() {
    let (state, _backend) = common::build_state("http://noop", common::args());
    let app = common::build_app(state, None);
    let res = app
        .oneshot(put_user("bob", r#"{"password":"pw"}"#, &basic("x:y")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
}
