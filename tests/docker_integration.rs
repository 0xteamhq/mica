//! Real-Docker integration tests for `DockerBackend`. Gated behind
//! `MICA_DOCKER_TESTS=1` and `#[ignore]` so CI without Docker stays
//! green. Run locally with:
//!
//! ```bash
//! MICA_DOCKER_TESTS=1 cargo test --test docker_integration -- --ignored
//! ```

use mica::backend::docker::DockerBackend;
use mica::backend::{Backend, BackendError, StartParams};
use mica::caps::Caps;
use mica::config::Config;
use std::time::Duration;

fn skip_unless_enabled() -> bool {
    std::env::var("MICA_DOCKER_TESTS").is_err()
}

fn params(browser: &str, version: Option<&str>) -> StartParams {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let (b, v) = cfg.find(browser, version).expect("found in fixture");
    StartParams {
        request_id: "rid-it".into(),
        caps: Caps::default(),
        browser: b,
        version: v,
    }
}

#[tokio::test]
#[ignore = "requires docker daemon (set MICA_DOCKER_TESTS=1)"]
async fn connects_to_daemon() {
    if skip_unless_enabled() {
        return;
    }
    let backend = DockerBackend::connect().await.expect("connect");
    backend.ping().await.expect("ping");
}

#[tokio::test]
#[ignore = "requires docker daemon (set MICA_DOCKER_TESTS=1)"]
async fn start_and_stop_browser_container() {
    if skip_unless_enabled() {
        return;
    }
    let backend = DockerBackend::connect()
        .await
        .expect("connect")
        .with_service_startup_timeout(Duration::from_secs(60));

    let started = backend
        .start(params("firefox", Some("126.0")))
        .await
        .expect("start");
    assert!(started.upstream.starts_with("http://127.0.0.1:"));
    assert!(started.upstream.ends_with("/wd/hub"));
    assert!(!started.container_id.is_empty());
    started.stop().await;
}

#[tokio::test]
#[ignore = "requires docker daemon (set MICA_DOCKER_TESTS=1)"]
async fn missing_image_surfaces_docker_error() {
    if skip_unless_enabled() {
        return;
    }
    let backend = DockerBackend::connect().await.expect("connect");
    // Construct a StartParams with a guaranteed-bogus image.
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let (mut browser, version) = cfg.find("firefox", None).unwrap();
    browser.image = serde_json::Value::String("mica/does-not-exist:0.0.0".into());
    let p = StartParams {
        request_id: "rid-bad".into(),
        caps: Caps::default(),
        browser,
        version,
    };
    match backend.start(p).await {
        Err(BackendError::Docker(_)) => {}
        other => panic!("expected Docker error, got {other:?}"),
    }
}
