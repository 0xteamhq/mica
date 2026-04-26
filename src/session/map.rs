use crate::session::Session;
use dashmap::DashMap;
use std::sync::Arc;

/// Concurrent session map.
///
/// Backed by `DashMap` so reads (proxy hot-path: look up + touch) don't
/// block writes (create / remove). On `remove()` we stop the idle
/// watcher and run the per-session cancel hook before dropping the
/// `Session` value.
#[derive(Clone, Default)]
pub struct SessionMap {
    inner: Arc<DashMap<String, Session>>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn put(&self, s: Session) {
        self.inner.insert(s.id().into(), s);
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.inner.get(id).map(|r| r.clone())
    }

    pub fn touch(&self, id: &str) {
        if let Some(s) = self.get(id) {
            s.touch();
        }
    }

    pub async fn remove(&self, id: &str) {
        if let Some((_, s)) = self.inner.remove(id) {
            s.stop_idle();
            s.run_cancel();
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn each<F: FnMut(&Session)>(&self, mut f: F) {
        for r in self.inner.iter() {
            f(r.value());
        }
    }
}
