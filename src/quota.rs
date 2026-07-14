//! Per-user session quotas (M3).
//!
//! GGR-style semantics on top of the single global semaphore: each
//! authenticated user has a concurrent-session limit checked BEFORE
//! `queue.acquire()`, so an over-quota request fails fast instead of
//! occupying a queue slot. The global `--limit` cap is unchanged and
//! `queue.rs` stays untouched.
//!
//! Config file (`--quotas`, hot-reloadable via SIGHUP and the admin
//! API): `{"default": 0, "users": {"alice": 3}}` — `0` = unlimited.
//! Sessions created with auth disabled have no owner and bypass
//! quotas entirely.

use arc_swap::ArcSwap;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Quotas {
    /// Limit for users without an explicit row. 0 = unlimited.
    #[serde(default)]
    pub default: usize,
    #[serde(default)]
    pub users: HashMap<String, usize>,
}

impl Quotas {
    /// Load from `path`. Empty path → default (quotas disabled).
    pub fn load(path: &str) -> Result<Self, String> {
        if path.is_empty() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("io: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("json: {e}"))
    }

    pub fn limit_for(&self, user: &str) -> usize {
        self.users.get(user).copied().unwrap_or(self.default)
    }
}

/// Live quota accounting. Limits hot-swap on reload; counts persist
/// across reloads (they track actual running sessions, not config).
#[derive(Clone, Default)]
pub struct QuotaTable {
    limits: Arc<ArcSwap<Quotas>>,
    counts: Arc<DashMap<String, Arc<AtomicUsize>>>,
}

/// Holds one unit of a user's quota; dropping it releases the unit.
/// Owned by the session's cancel hook so its lifetime exactly matches
/// the session's queue slot.
pub struct QuotaGuard {
    count: Arc<AtomicUsize>,
}

impl Drop for QuotaGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

impl QuotaTable {
    pub fn new(quotas: Quotas) -> Self {
        Self {
            limits: Arc::new(ArcSwap::from_pointee(quotas)),
            counts: Arc::default(),
        }
    }

    pub fn snapshot(&self) -> Arc<Quotas> {
        self.limits.load_full()
    }

    pub fn store(&self, quotas: Quotas) {
        self.limits.store(Arc::new(quotas));
    }

    /// Current in-flight session count for a user (dashboard surface).
    pub fn in_use(&self, user: &str) -> usize {
        self.counts
            .get(user)
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// Reserve one unit of `user`'s quota, or `None` when the user is
    /// at their limit. Optimistic fetch_add with rollback keeps the
    /// check race-free without a lock.
    pub fn try_acquire(&self, user: &str) -> Option<QuotaGuard> {
        let limit = self.limits.load().limit_for(user);
        let count = self
            .counts
            .entry(user.to_string())
            .or_default()
            .value()
            .clone();
        let prev = count.fetch_add(1, Ordering::SeqCst);
        if limit > 0 && prev >= limit {
            count.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(QuotaGuard { count })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_by_default() {
        let t = QuotaTable::default();
        for _ in 0..100 {
            assert!(t.try_acquire("alice").is_some());
        }
    }

    #[test]
    fn limit_enforced_and_released_on_drop() {
        let t = QuotaTable::new(Quotas {
            default: 0,
            users: HashMap::from([("alice".to_string(), 2)]),
        });
        let g1 = t.try_acquire("alice").expect("1st");
        let _g2 = t.try_acquire("alice").expect("2nd");
        assert!(t.try_acquire("alice").is_none(), "3rd exceeds limit");
        assert_eq!(t.in_use("alice"), 2);
        drop(g1);
        assert!(t.try_acquire("alice").is_some(), "slot freed on drop");
        // Unlimited users are unaffected by alice's rows.
        assert!(t.try_acquire("bob").is_some());
    }

    #[test]
    fn default_limit_applies_to_unlisted_users() {
        let t = QuotaTable::new(Quotas {
            default: 1,
            users: HashMap::new(),
        });
        let _g = t.try_acquire("carol").expect("within default");
        assert!(t.try_acquire("carol").is_none());
    }
}
