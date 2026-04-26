//! Warm pool — keeps `min_idle` pre-started browser sandboxes per
//! `(image, screen_resolution, env_hash)` key so session creation
//! returns in ~300 ms instead of ~700 ms (cold image pull + init).
//!
//! The pool sits between `Queue` and `Backend`: `WarmPool::start`
//! either reuses a checked-out warm sandbox or falls through to
//! `backend.start`. After every checkout we eagerly refill in the
//! background up to `min_idle`. LRU eviction keeps the pool
//! bounded by `max_idle`.
//!
//! Pool entries hold a `StartedSession` whose `stopper` we'll call
//! either on idle eviction or when the session that ultimately uses
//! the entry is removed (the cancel hook in `handlers::create`).

use crate::backend::{Backend, BackendError, StartParams, StartedSession};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Identity of a warmed sandbox. Two sessions share an entry only
/// when image, screen resolution, and the sorted env share match.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PoolKey {
    pub image: String,
    pub screen_resolution: Option<String>,
    /// Sorted env vars, "K=V" each.
    pub env: Vec<String>,
}

impl PoolKey {
    pub fn from_params(p: &StartParams) -> Option<Self> {
        let image = p.browser.docker_image()?.to_string();
        let mut env: Vec<String> = p
            .browser
            .env
            .iter()
            .chain(p.caps.env.iter())
            .cloned()
            .collect();
        env.sort();
        Some(Self {
            image,
            screen_resolution: p.caps.screen_resolution.clone(),
            env,
        })
    }
}

struct Entry {
    started: StartedSession,
    inserted: Instant,
}

#[derive(Clone)]
pub struct WarmPool {
    inner: Arc<Mutex<HashMap<PoolKey, VecDeque<Entry>>>>,
    backend: Arc<dyn Backend>,
    min_idle: usize,
    max_idle: usize,
    idle_ttl: Duration,
}

impl WarmPool {
    pub fn new(
        backend: Arc<dyn Backend>,
        min_idle: usize,
        max_idle: usize,
        idle_ttl: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            backend,
            min_idle,
            max_idle,
            idle_ttl,
        }
    }

    /// Try to take a warm sandbox for `params`. Returns `None` if no
    /// matching entry is available; caller should fall back to
    /// `Backend::start`.
    pub async fn checkout(&self, params: &StartParams) -> Option<StartedSession> {
        let key = PoolKey::from_params(params)?;
        let mut g = self.inner.lock().await;
        let q = g.get_mut(&key)?;
        // Drop stale entries first (they'll be stopped in the
        // background to release Docker resources).
        while let Some(front) = q.front() {
            if front.inserted.elapsed() > self.idle_ttl {
                let stale = q.pop_front().expect("front exists");
                tokio::spawn(async move { stale.started.stop().await });
            } else {
                break;
            }
        }
        let entry = q.pop_front()?;
        Some(entry.started)
    }

    /// Background top-up: ensure the pool has `min_idle` entries for
    /// `params`'s key. Fire-and-forget; failures log + retry on the
    /// next checkout.
    pub fn refill(&self, params: StartParams) {
        let pool = self.clone();
        tokio::spawn(async move {
            let key = match PoolKey::from_params(&params) {
                Some(k) => k,
                None => return,
            };
            loop {
                let need = {
                    let g = pool.inner.lock().await;
                    let have = g.get(&key).map(VecDeque::len).unwrap_or(0);
                    pool.min_idle.saturating_sub(have)
                };
                if need == 0 {
                    return;
                }
                match pool.backend.start(clone_params(&params)).await {
                    Ok(started) => {
                        let mut g = pool.inner.lock().await;
                        let q = g.entry(key.clone()).or_default();
                        q.push_back(Entry {
                            started,
                            inserted: Instant::now(),
                        });
                        // LRU eviction past max_idle.
                        while q.len() > pool.max_idle {
                            if let Some(stale) = q.pop_front() {
                                tokio::spawn(async move { stale.started.stop().await });
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "warm-pool refill failed");
                        return;
                    }
                }
            }
        });
    }

    /// Tear down every warmed entry — call from graceful shutdown.
    pub async fn drain(&self) {
        let mut g = self.inner.lock().await;
        let entries: Vec<Entry> = g.drain().flat_map(|(_, q)| q.into_iter()).collect();
        drop(g);
        for e in entries {
            e.started.stop().await;
        }
    }
}

/// `StartParams` is intentionally not `Clone` (the Backend trait owns
/// it). For pool refills we have to construct a fresh one from the
/// pieces we kept.
fn clone_params(p: &StartParams) -> StartParams {
    StartParams {
        request_id: format!("warm-{}", uuid::Uuid::new_v4()),
        caps: p.caps.clone(),
        browser: p.browser.clone(),
        version: p.version.clone(),
    }
}

/// `Backend` impl that wraps another backend with pool semantics.
/// `start` first tries `pool.checkout`; on miss, calls inner backend.
/// Either way, kicks `refill` so the pool catches up.
pub struct PooledBackend {
    inner: Arc<dyn Backend>,
    pool: WarmPool,
}

impl PooledBackend {
    pub fn new(
        inner: Arc<dyn Backend>,
        min_idle: usize,
        max_idle: usize,
        idle_ttl: Duration,
    ) -> Self {
        let pool = WarmPool::new(inner.clone(), min_idle, max_idle, idle_ttl);
        Self { inner, pool }
    }

    pub fn pool(&self) -> &WarmPool {
        &self.pool
    }
}

#[async_trait]
impl Backend for PooledBackend {
    async fn start(&self, params: StartParams) -> Result<StartedSession, BackendError> {
        if let Some(s) = self.pool.checkout(&params).await {
            self.pool.refill(clone_params(&params));
            return Ok(s);
        }
        let res = self.inner.start(clone_params(&params)).await;
        self.pool.refill(params);
        res
    }
}
