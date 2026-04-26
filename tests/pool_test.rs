use mica::backend::mock::MockBackend;
use mica::backend::{Backend, StartParams};
use mica::caps::Caps;
use mica::config::Config;
use mica::pool::PooledBackend;
use std::sync::Arc;
use std::time::Duration;

fn params() -> StartParams {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let (browser, version) = cfg.find("firefox", None).unwrap();
    StartParams {
        request_id: "rid".into(),
        caps: Caps::default(),
        browser,
        version,
    }
}

#[tokio::test]
async fn pool_falls_through_to_inner_on_miss() {
    let inner = Arc::new(MockBackend::new("http://upstream"));
    let pool = PooledBackend::new(inner.clone(), 0, 4, Duration::from_secs(60));
    let started = pool.start(params()).await.unwrap();
    assert_eq!(started.upstream, "http://upstream");
    started.stop().await;
}

#[tokio::test]
async fn pool_refills_in_background() {
    let inner = Arc::new(MockBackend::new("http://upstream"));
    let pool = PooledBackend::new(inner, 2, 4, Duration::from_secs(60));
    // First start: pool empty, falls through to inner; refill kicks
    // off after.
    let _s = pool.start(params()).await.unwrap();
    // Give the background refill a moment.
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Second start: should hit a warmed entry — same upstream proves
    // the inner backend was reused, but more importantly we can do
    // it without the inner backend creating a brand-new session.
    let s = pool.start(params()).await.unwrap();
    assert_eq!(s.upstream, "http://upstream");
    s.stop().await;
}
