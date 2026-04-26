//! Bounded session queue mirroring `selenoid/protect/queue.go:15-82`.
//!
//! Selenoid uses four channels (`limit`, `queued`, `pending`, `used`) to
//! gate parallel sessions; we use a single tokio semaphore for the hard
//! capacity bound and three atomics for the observable counters
//! (`queued`, `pending`, `used`) that `/status` and `/ping` v2 will surface.
//!
//! ## Lifecycle
//!
//! - `try_acquire()` — non-blocking, returns `None` when full. Increments
//!   `pending`. Wired up to the create-session handler when the
//!   `X-Selenoid-No-Wait: 1` header is set or `--disable-queue` is passed
//!   (see `handlers::create`). This is the contract behind T14.
//! - `acquire().await` — the default path. Waits for a slot. Increments
//!   `queued` while waiting, then decrements `queued` and increments
//!   `pending` once a slot opens.
//! - `Permit::promote()` — call once the WebDriver session is live to
//!   move the slot from `pending` to `used`. Idempotent.
//! - drop(permit) — releases the slot and the corresponding counter
//!   (`pending` if not yet promoted, otherwise `used`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// One reservation against the queue. Drop releases the slot.
pub struct Permit {
    _inner: OwnedSemaphorePermit,
    used: Arc<AtomicUsize>,
    pending: Arc<AtomicUsize>,
    promoted: bool,
}

impl Permit {
    /// Move the permit from `pending` to `used` once the WebDriver
    /// session has been created upstream. Idempotent.
    pub fn promote(&mut self) {
        if !self.promoted {
            self.pending.fetch_sub(1, Ordering::SeqCst);
            self.used.fetch_add(1, Ordering::SeqCst);
            self.promoted = true;
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if self.promoted {
            self.used.fetch_sub(1, Ordering::SeqCst);
        } else {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[derive(Clone)]
pub struct Queue {
    sem: Arc<Semaphore>,
    capacity: usize,
    used: Arc<AtomicUsize>,
    pending: Arc<AtomicUsize>,
    queued: Arc<AtomicUsize>,
}

impl Queue {
    pub fn new(capacity: u32) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(capacity as usize)),
            capacity: capacity as usize,
            used: Arc::new(AtomicUsize::new(0)),
            pending: Arc::new(AtomicUsize::new(0)),
            queued: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Non-blocking acquire. Returns `None` when the queue is full.
    pub fn try_acquire(&self) -> Option<Permit> {
        let permit = self.sem.clone().try_acquire_owned().ok()?;
        self.pending.fetch_add(1, Ordering::SeqCst);
        Some(Permit {
            _inner: permit,
            used: self.used.clone(),
            pending: self.pending.clone(),
            promoted: false,
        })
    }

    /// Blocking acquire. Increments `queued` while waiting for a slot.
    pub async fn acquire(&self) -> Permit {
        self.queued.fetch_add(1, Ordering::SeqCst);
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore is never closed");
        self.queued.fetch_sub(1, Ordering::SeqCst);
        self.pending.fetch_add(1, Ordering::SeqCst);
        Permit {
            _inner: permit,
            used: self.used.clone(),
            pending: self.pending.clone(),
            promoted: false,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }
    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }
    pub fn queued(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }
}
