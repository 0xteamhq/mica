//! Session lifecycle types — `Session` and `SessionMap`.
//!
//! `DashMap` for the concurrent map; a per-session tokio task drives
//! the idle watcher, with a oneshot channel to cancel that watcher
//! when the session is removed.
//!
//! Two callbacks are wired through the `Session`:
//!
//! - `on_idle` (`Fn`) — fired once when `last_seen` is older than
//!   `idle_timeout`. The watcher exits after firing.
//! - `cancel` (`FnOnce`) — fired exactly once when the session is
//!   removed from the map. Used by `handlers::create` to release queue
//!   permits, kill the upstream container, and emit `SessionStopped`.

pub mod map;

use crate::backend::HostPorts;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use map::SessionMap;

type IdleCallback = Box<dyn Fn() + Send + Sync + 'static>;
type CancelCallback = Box<dyn FnOnce() + Send + 'static>;

struct SessionInner {
    id: String,
    upstream: String,
    host_ports: HostPorts,
    browser_name: String,
    browser_version: String,
    /// Authenticated user who created the session (None when auth is
    /// disabled). Shown on /admin/api/state, deliberately NOT on
    /// /status — every authenticated user can read /status and the
    /// user↔session mapping shouldn't leak there.
    owner: Option<String>,
    last_seen: Mutex<Instant>,
    idle_timeout: Duration,
    on_idle: Option<IdleCallback>,
    cancel: Mutex<Option<CancelCallback>>,
    cancel_idle: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub started: std::time::SystemTime,
}

#[derive(Clone)]
pub struct Session(Arc<SessionInner>);

impl Session {
    /// Bare session for tests. No idle watcher, no cancel hook.
    pub fn new_for_test(id: &str, upstream: String) -> Self {
        Self::new_inner(id, upstream, Duration::from_secs(60), None, None)
    }

    /// Session with an idle watcher. Fires `cb` once when no `touch()`
    /// arrives within `idle`.
    pub fn new_with_idle(id: &str, upstream: String, idle: Duration, cb: IdleCallback) -> Self {
        let s = Self::new_inner(id, upstream, idle, Some(cb), None);
        s.spawn_idle_watcher();
        s
    }

    /// Session with a cancel hook (no idle watcher). `cancel` runs
    /// exactly once when the session is removed from the map.
    pub fn new_with_cancel(id: &str, upstream: String, cancel: CancelCallback) -> Self {
        Self::new_inner(id, upstream, Duration::from_secs(60), None, Some(cancel))
    }

    /// Full-featured session: idle watcher fires `on_idle` after
    /// `idle` of inactivity, and `cancel` runs exactly once on
    /// `SessionMap::remove`.
    pub fn new_with_idle_and_cancel(
        id: &str,
        upstream: String,
        idle: Duration,
        on_idle: IdleCallback,
        cancel: CancelCallback,
    ) -> Self {
        let s = Self::new_inner(id, upstream, idle, Some(on_idle), Some(cancel));
        s.spawn_idle_watcher();
        s
    }

    /// Production-shaped session. Same wiring as
    /// `new_with_idle_and_cancel` plus the extra metadata `/status`
    /// and `/vnc` need (host ports, browser name + version).
    #[allow(clippy::too_many_arguments)]
    pub fn new_full(
        id: &str,
        upstream: String,
        host_ports: HostPorts,
        browser_name: impl Into<String>,
        browser_version: impl Into<String>,
        owner: Option<String>,
        idle: Duration,
        on_idle: IdleCallback,
        cancel: CancelCallback,
    ) -> Self {
        let inner = SessionInner {
            id: id.into(),
            upstream,
            host_ports,
            browser_name: browser_name.into(),
            browser_version: browser_version.into(),
            owner,
            last_seen: Mutex::new(Instant::now()),
            idle_timeout: idle,
            on_idle: Some(on_idle),
            cancel: Mutex::new(Some(cancel)),
            cancel_idle: Mutex::new(None),
            started: std::time::SystemTime::now(),
        };
        let s = Self(Arc::new(inner));
        s.spawn_idle_watcher();
        s
    }

    fn new_inner(
        id: &str,
        upstream: String,
        idle: Duration,
        on_idle: Option<IdleCallback>,
        cancel: Option<CancelCallback>,
    ) -> Self {
        Self(Arc::new(SessionInner {
            id: id.into(),
            upstream,
            host_ports: HostPorts::default(),
            browser_name: String::new(),
            browser_version: String::new(),
            owner: None,
            last_seen: Mutex::new(Instant::now()),
            idle_timeout: idle,
            on_idle,
            cancel: Mutex::new(cancel),
            cancel_idle: Mutex::new(None),
            started: std::time::SystemTime::now(),
        }))
    }

    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn upstream(&self) -> &str {
        &self.0.upstream
    }
    pub fn host_ports(&self) -> &HostPorts {
        &self.0.host_ports
    }
    pub fn browser_name(&self) -> &str {
        &self.0.browser_name
    }
    pub fn browser_version(&self) -> &str {
        &self.0.browser_version
    }
    pub fn owner(&self) -> Option<&str> {
        self.0.owner.as_deref()
    }
    pub fn started(&self) -> std::time::SystemTime {
        self.0.started
    }

    /// Reset the idle clock.
    pub fn touch(&self) {
        *self.0.last_seen.lock().unwrap() = Instant::now();
    }

    fn spawn_idle_watcher(&self) {
        let s = self.clone();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        *self.0.cancel_idle.lock().unwrap() = Some(tx);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(20));
            loop {
                tokio::select! {
                    _ = &mut rx => return,
                    _ = ticker.tick() => {
                        let last = *s.0.last_seen.lock().unwrap();
                        if last.elapsed() >= s.0.idle_timeout
                            && let Some(cb) = s.0.on_idle.as_ref() {
                                cb();
                                return;
                            }
                    }
                }
            }
        });
    }

    /// Stop the idle watcher (no-op if none was spawned). Called by
    /// `SessionMap::remove`.
    pub(crate) fn stop_idle(&self) {
        if let Some(tx) = self.0.cancel_idle.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    /// Run the cancel hook if one was registered. Idempotent — the
    /// callback is taken out of the slot, so subsequent calls do
    /// nothing. Called by `SessionMap::remove`.
    pub(crate) fn run_cancel(&self) {
        if let Some(cb) = self.0.cancel.lock().unwrap().take() {
            cb();
        }
    }
}
