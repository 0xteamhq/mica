use mica::queue::Queue;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn try_acquire_when_under_limit() {
    let q = Queue::new(2);
    let p1 = q.try_acquire().expect("under limit");
    let p2 = q.try_acquire().expect("under limit");
    assert!(q.try_acquire().is_none(), "third must be rejected");
    drop(p1);
    assert!(q.try_acquire().is_some(), "slot freed");
    drop(p2);
}

#[tokio::test]
async fn acquire_blocks_until_slot_free() {
    let q = Arc::new(Queue::new(1));
    let permit = q.acquire().await;
    let q2 = q.clone();
    let h = tokio::spawn(async move { q2.acquire().await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!h.is_finished(), "must still be waiting");
    drop(permit);
    let _ = tokio::time::timeout(Duration::from_secs(1), h)
        .await
        .expect("must complete");
}

#[tokio::test]
async fn promote_moves_pending_to_used() {
    let q = Queue::new(3);
    let mut p = q.acquire().await;
    assert_eq!(q.pending(), 1);
    assert_eq!(q.used(), 0);
    p.promote();
    assert_eq!(q.pending(), 0);
    assert_eq!(q.used(), 1);
    drop(p);
    assert_eq!(q.pending(), 0);
    assert_eq!(q.used(), 0);
}

#[tokio::test]
async fn used_count_tracks_active_sessions() {
    let q = Queue::new(3);
    let mut p1 = q.acquire().await;
    let mut p2 = q.acquire().await;
    p1.promote();
    p2.promote();
    assert_eq!(q.used(), 2);
    assert_eq!(q.pending(), 0);
}

#[tokio::test]
async fn capacity_reports_configured_value() {
    let q = Queue::new(7);
    assert_eq!(q.capacity(), 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stress_never_exceeds_capacity() {
    let q = Arc::new(Queue::new(8));
    let mut handles = vec![];
    for _ in 0..200 {
        let q = q.clone();
        handles.push(tokio::spawn(async move {
            let p = q.acquire().await;
            tokio::time::sleep(Duration::from_millis(2)).await;
            assert!(q.used() + q.pending() <= 8);
            drop(p);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(q.used(), 0);
    assert_eq!(q.pending(), 0);
    assert_eq!(q.queued(), 0);
}
