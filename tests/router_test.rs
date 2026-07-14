//! M4 router mode: placement, failover, stateless proxying, and the
//! aggregated /status — driven against wiremock "nodes".

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::Parser;
use mica::cli::Args;
use mica::router::registry::{NodeState, NodesConfig, Registry};
use mica::router::{RouterState, health, session_id};
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::matchers::{body_json_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FF_BODY: &str = r#"{"capabilities":{"alwaysMatch":{"browserName":"firefox"}}}"#;

fn routed(node: &str, id: &str) -> String {
    session_id::encode(node, id)
}

/// nodes.json on disk → RouterState + axum Router, health un-polled.
fn build_router(nodes_json: &str) -> (tempfile::NamedTempFile, RouterState, axum::Router) {
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), nodes_json).unwrap();
    let args = Args::parse_from([
        "mica",
        "--router",
        "--nodes",
        f.path().to_str().unwrap(),
        "--router-max-attempts",
        "3",
        "--router-create-timeout",
        "10s",
    ]);
    let registry = Arc::new(Registry::new(
        NodesConfig::load(f.path().to_str().unwrap()).unwrap(),
    ));
    let state = RouterState {
        registry,
        args: Arc::new(args),
        http: reqwest::Client::new(),
        metrics: None,
    };
    let app = mica::router::router(state.clone());
    (f, state, app)
}

async fn poll(state: &RouterState) {
    health::poll_once(
        &state.registry,
        &state.http,
        std::time::Duration::from_secs(2),
        2,
    )
    .await;
}

/// Mount a /status advertising `browsers` on a node mock.
async fn mount_status(node: &MockServer, browsers: serde_json::Value, sessions: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total": 5, "used": 1, "queued": 0, "pending": 0,
            "browsers": browsers, "sessions": sessions
        })))
        .mount(node)
        .await;
}

fn two_node_config(a: &MockServer, b: &MockServer) -> String {
    serde_json::json!({
        "nodes": [
            { "name": "node-a", "endpoint": a.uri() },
            { "name": "node-b", "endpoint": b.uri() },
        ]
    })
    .to_string()
}

async fn post_create(app: &axum::Router) -> (StatusCode, serde_json::Value) {
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
    let status = res.status();
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn create_routes_by_capability_and_rewrites_session_id() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    // Only node-b advertises firefox — placement must land there.
    mount_status(
        &a,
        serde_json::json!({"chrome": ["126.0"]}),
        serde_json::json!([]),
    )
    .await;
    mount_status(
        &b,
        serde_json::json!({"firefox": ["126.0"]}),
        serde_json::json!([]),
    )
    .await;
    // Body must arrive byte-identical.
    Mock::given(method("POST"))
        .and(path("/wd/hub/session"))
        .and(body_json_string(FF_BODY))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "up-1", "capabilities": {} }
        })))
        .expect(1)
        .mount(&b)
        .await;

    let (_f, state, app) = build_router(&two_node_config(&a, &b));
    poll(&state).await;

    let (status, json) = post_create(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["value"]["sessionId"].as_str().unwrap(),
        routed("node-b", "up-1")
    );
    // Prefix is legit base64url of the node name.
    let prefix = json["value"]["sessionId"]
        .as_str()
        .unwrap()
        .split_once('.')
        .unwrap()
        .0
        .to_string();
    assert_eq!(URL_SAFE_NO_PAD.decode(prefix).unwrap(), b"node-b");
}

#[tokio::test]
async fn create_fails_over_on_5xx_but_not_on_4xx() {
    // Failover: the failing node 500s, the healthy one answers.
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    let ff = serde_json::json!({"firefox": ["126.0"]});
    mount_status(&a, ff.clone(), serde_json::json!([])).await;
    mount_status(&b, ff.clone(), serde_json::json!([])).await;
    Mock::given(method("POST"))
        .and(path("/wd/hub/session"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .and(path("/wd/hub/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": { "sessionId": "up-ok", "capabilities": {} }
        })))
        .mount(&b)
        .await;

    let (_f, state, app) = build_router(&two_node_config(&a, &b));
    poll(&state).await;
    let (status, json) = post_create(&app).await;
    assert_eq!(status, StatusCode::OK, "failover reached the healthy node");
    assert_eq!(
        json["value"]["sessionId"].as_str().unwrap(),
        routed("node-b", "up-ok")
    );

    // 4xx is returned verbatim: exactly one attempt, no retry.
    let c = MockServer::start().await;
    mount_status(&c, ff, serde_json::json!([])).await;
    Mock::given(method("POST"))
        .and(path("/wd/hub/session"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "value": { "error": "invalid argument", "message": "bad caps" }
        })))
        .expect(1)
        .mount(&c)
        .await;
    let cfg = serde_json::json!({"nodes": [{ "name": "node-c", "endpoint": c.uri() }]}).to_string();
    let (_f2, state2, app2) = build_router(&cfg);
    poll(&state2).await;
    let (status, json) = post_create(&app2).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR); // W3C session not created
    assert!(
        json["value"]["message"]
            .as_str()
            .unwrap()
            .contains("bad caps"),
        "node's message passes through: {json}"
    );
    c.verify().await; // expect(1): no second attempt
}

#[tokio::test]
async fn weight_zero_and_unpolled_nodes_are_not_placed() {
    let a = MockServer::start().await;
    mount_status(
        &a,
        serde_json::json!({"firefox": ["126.0"]}),
        serde_json::json!([]),
    )
    .await;
    let cfg = serde_json::json!({
        "nodes": [{ "name": "node-a", "endpoint": a.uri(), "weight": 0 }]
    })
    .to_string();
    let (_f, state, app) = build_router(&cfg);
    poll(&state).await;
    let (status, json) = post_create(&app).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json["value"]["message"]
            .as_str()
            .unwrap()
            .contains("no healthy node"),
        "{json}"
    );

    // Same story before the first successful poll (state Unknown).
    let (_f2, _state2, app2) = build_router(&two_node_config(&a, &a));
    let (status, _) = post_create(&app2).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn proxy_decodes_prefix_and_injects_node_auth() {
    let a = MockServer::start().await;
    mount_status(&a, serde_json::json!({}), serde_json::json!([])).await;
    // Node must see the BARE id and the ROUTER's node credentials —
    // never the client's Authorization.
    let expected_auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(b"router:s3cret")
    );
    Mock::given(method("GET"))
        .and(path("/wd/hub/session/up-9/url"))
        .and(header("authorization", expected_auth.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": "https://example.com"
        })))
        .expect(1)
        .mount(&a)
        .await;

    let cfg = serde_json::json!({
        "nodes": [{
            "name": "node-a", "endpoint": a.uri(),
            "username": "router", "password": "s3cret"
        }]
    })
    .to_string();
    let (_f, _state, app) = build_router(&cfg);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/wd/hub/session/{}/url", routed("node-a", "up-9")))
                .header(
                    axum::http::header::AUTHORIZATION,
                    "Basic Y2xpZW50OmNyZWRz", // client:creds — must be stripped
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    a.verify().await;

    // Garbage / node-issued ids don't route.
    for bad in ["not-routed-uuid", "!!!.id", &routed("ghost-node", "x")] {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/wd/hub/session/{bad}/url"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{bad}");
    }
}

#[tokio::test]
async fn artifacts_route_through_the_name_prefix() {
    let a = MockServer::start().await;
    mount_status(&a, serde_json::json!({}), serde_json::json!([])).await;
    Mock::given(method("GET"))
        .and(path("/video/up-7.mp4"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"MP4".to_vec()))
        .expect(1)
        .mount(&a)
        .await;

    let cfg = serde_json::json!({"nodes": [{ "name": "node-a", "endpoint": a.uri() }]}).to_string();
    let (_f, _state, app) = build_router(&cfg);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/video/{}.mp4", routed("node-a", "up-7")))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"MP4");
    a.verify().await;
}

#[tokio::test]
async fn aggregated_status_merges_and_annotates() {
    let a = MockServer::start().await;
    mount_status(
        &a,
        serde_json::json!({"chrome": ["126.0"], "firefox": ["126.0"]}),
        serde_json::json!([{"id": "up-1", "browser": "chrome", "version": "126.0", "started": "t"}]),
    )
    .await;
    // node-b's endpoint always refuses: port 1 is privileged and
    // unbound, and (unlike a dropped MockServer's port) can't be
    // grabbed by a concurrent test's server.
    let cfg = serde_json::json!({
        "nodes": [
            { "name": "node-a", "endpoint": a.uri(), "region": "us-east-1" },
            { "name": "node-b", "endpoint": "http://127.0.0.1:1" },
        ]
    })
    .to_string();
    let (_f, state, app) = build_router(&cfg);
    poll(&state).await;
    poll(&state).await; // second failure crosses the unhealthy threshold

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["router"], true);
    assert_eq!(json["total"], 5, "only healthy node-a counts");
    assert_eq!(json["used"], 1);
    assert_eq!(json["browsers"]["chrome"][0], "126.0");
    // Session id re-encoded with the router prefix + node annotation.
    assert_eq!(
        json["sessions"][0]["id"].as_str().unwrap(),
        routed("node-a", "up-1")
    );
    assert_eq!(json["sessions"][0]["node"], "node-a");

    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    let node_b = nodes.iter().find(|n| n["name"] == "node-b").unwrap();
    assert_eq!(node_b["state"], "unhealthy");
    assert!(node_b["error"].is_string());
    assert_eq!(node_b["used"], 0);
    let node_a = nodes.iter().find(|n| n["name"] == "node-a").unwrap();
    assert_eq!(node_a["state"], "healthy");
    assert_eq!(node_a["region"], "us-east-1");

    // Health state machine sanity via the registry.
    assert_eq!(
        state.registry.dynamic("node-b").unwrap().state,
        NodeState::Unhealthy
    );
    assert_eq!(state.registry.healthy_count(), 1);
}

#[tokio::test]
async fn readyz_reflects_healthy_node_count() {
    // Endpoint that always refuses (see aggregated_status test).
    let cfg =
        serde_json::json!({"nodes": [{ "name": "node-a", "endpoint": "http://127.0.0.1:1" }]})
            .to_string();
    let (_f, state, app) = build_router(&cfg);
    poll(&state).await;
    poll(&state).await;

    let res = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn draining_node_is_proxied_but_not_placed() {
    let a = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total": 5, "used": 0, "queued": 0, "pending": 0, "draining": true,
            "browsers": {"firefox": ["126.0"]}, "sessions": []
        })))
        .mount(&a)
        .await;
    Mock::given(method("GET"))
        .and(path("/wd/hub/session/up-3/url"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": "x"})))
        .mount(&a)
        .await;

    let cfg = serde_json::json!({"nodes": [{ "name": "node-a", "endpoint": a.uri() }]}).to_string();
    let (_f, state, app) = build_router(&cfg);
    poll(&state).await;
    assert_eq!(
        state.registry.dynamic("node-a").unwrap().state,
        NodeState::Draining
    );

    // No placement…
    let (status, json) = post_create(&app).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json["value"]["message"]
            .as_str()
            .unwrap()
            .contains("no healthy node")
    );

    // …but existing sessions still proxy.
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/wd/hub/session/{}/url", routed("node-a", "up-3")))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

/// WS relay through real listeners (oneshot can't upgrade): client ⇄
/// router ⇄ node echo, both directions, through /session/{id}/bidi.
#[tokio::test]
async fn ws_bidi_relays_frames_through_the_router() {
    use futures::{SinkExt, StreamExt};

    // "Node": echoes every text frame back.
    async fn echo(ws: axum::extract::ws::WebSocketUpgrade) -> axum::response::Response {
        ws.on_upgrade(|mut sock| async move {
            while let Some(Ok(msg)) = sock.recv().await {
                if let axum::extract::ws::Message::Text(t) = msg {
                    let _ = sock
                        .send(axum::extract::ws::Message::Text(format!("echo:{t}")))
                        .await;
                }
            }
        })
    }
    let node_app = axum::Router::new().route("/session/:id/bidi", axum::routing::get(echo));
    let node_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node_addr = node_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(node_listener, node_app).into_future());

    let cfg = serde_json::json!({
        "nodes": [{ "name": "node-a", "endpoint": format!("http://{node_addr}") }]
    })
    .to_string();
    let (_f, _state, app) = build_router(&cfg);
    let router_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let router_addr = router_listener.local_addr().unwrap();
    tokio::spawn(axum::serve(router_listener, app).into_future());

    let url = format!(
        "ws://{router_addr}/session/{}/bidi",
        routed("node-a", "up-ws")
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("upgrade via router");
    ws.send(tokio_tungstenite::tungstenite::Message::Text("ping".into()))
        .await
        .unwrap();
    let reply = tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
        .await
        .expect("timely echo")
        .expect("frame")
        .expect("ok");
    assert_eq!(
        reply,
        tokio_tungstenite::tungstenite::Message::Text("echo:ping".into())
    );
}
