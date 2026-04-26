use mica::session::{Session, SessionMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;

#[tokio::test]
async fn put_get_remove() {
    let map = SessionMap::new();
    let s = Session::new_for_test("abc", "http://localhost:9000".into());
    map.put(s).await;
    let got = map.get("abc").expect("present");
    assert_eq!(got.id(), "abc");
    assert_eq!(got.upstream(), "http://localhost:9000");
    assert_eq!(map.len(), 1);
    map.remove("abc").await;
    assert!(map.get("abc").is_none());
    assert_eq!(map.len(), 0);
}

#[tokio::test]
async fn touch_resets_idle() {
    let map = SessionMap::new();
    let fired = Arc::new(Notify::new());
    let s = Session::new_with_idle("ab", "http://x".into(), Duration::from_millis(100), {
        let f = fired.clone();
        Box::new(move || f.notify_one())
    });
    map.put(s).await;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        map.touch("ab");
    }
    // After 250 ms with touches every 50 ms, idle must NOT have fired.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), fired.notified())
            .await
            .is_err(),
        "idle fired despite touches"
    );
    map.remove("ab").await;
}

#[tokio::test]
async fn idle_fires_when_not_touched() {
    let map = SessionMap::new();
    let fired = Arc::new(Notify::new());
    let s = Session::new_with_idle("ab", "http://x".into(), Duration::from_millis(50), {
        let f = fired.clone();
        Box::new(move || f.notify_one())
    });
    map.put(s).await;
    tokio::time::timeout(Duration::from_millis(500), fired.notified())
        .await
        .expect("idle must fire");
}

#[tokio::test]
async fn cancel_hook_runs_on_remove() {
    let map = SessionMap::new();
    let count = Arc::new(AtomicUsize::new(0));
    let count2 = count.clone();
    let s = Session::new_with_cancel(
        "x",
        "http://up".into(),
        Box::new(move || {
            count2.fetch_add(1, Ordering::SeqCst);
        }),
    );
    map.put(s).await;
    map.remove("x").await;
    assert_eq!(count.load(Ordering::SeqCst), 1, "cancel must run once");
    // Removing again is a no-op; cancel must not run twice.
    map.remove("x").await;
    assert_eq!(count.load(Ordering::SeqCst), 1, "cancel must not re-run");
}
