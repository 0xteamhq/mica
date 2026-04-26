use mica::backend::mock::MockBackend;
use mica::backend::{Backend, BackendError, StartParams};
use mica::caps::Caps;
use mica::config::Config;
use mica::error::WdError;

fn params(version: &str) -> StartParams {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let (browser, version) = cfg.find("firefox", Some(version)).unwrap();
    StartParams {
        request_id: "rid-1".into(),
        caps: Caps::default(),
        browser,
        version,
    }
}

#[tokio::test]
async fn mock_backend_returns_url() {
    let backend = MockBackend::new("http://upstream:4444");
    let started = backend.start(params("126.0")).await.expect("start");
    assert_eq!(started.upstream, "http://upstream:4444");
    assert_eq!(started.container_id, "mock");
    started.stop().await;
}

#[tokio::test]
async fn mock_backend_failure_propagates() {
    let backend = MockBackend::failing("daemon unreachable");
    let err = backend.start(params("126.0")).await.unwrap_err();
    match err {
        BackendError::Docker(msg) => assert!(msg.contains("daemon unreachable")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn backend_error_maps_to_wd_error() {
    let cases = [
        BackendError::Docker("image not found".into()),
        BackendError::Timeout,
        BackendError::Other("boom".into()),
    ];
    for err in cases {
        let original_msg = err.to_string();
        let wd: WdError = err.into();
        assert_eq!(wd.value.error, "session not created");
        assert!(
            wd.value.message.contains(&original_msg) || !original_msg.is_empty(),
            "message must surface backend cause"
        );
    }
}
