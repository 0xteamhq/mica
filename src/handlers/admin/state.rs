//! GET /admin/api/state — full dashboard snapshot.
//!
//! A superset of /status aimed at the bundled UI: per-session
//! capability flags (vnc/devtools/logs), session ownership (M2), the
//! draining flag, and the browser registry with default versions.

use crate::auth::RequireAdmin;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[derive(Serialize)]
pub struct StateResponse {
    pub capacity: Capacity,
    pub draining: bool,
    pub browsers: BTreeMap<String, BrowserInfo>,
    pub sessions: Vec<SessionInfo>,
}

#[derive(Serialize)]
pub struct Capacity {
    pub total: usize,
    pub used: usize,
    pub queued: usize,
    pub pending: usize,
}

#[derive(Serialize)]
pub struct BrowserInfo {
    pub default: String,
    pub versions: Vec<String>,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub browser: String,
    pub version: String,
    pub started: String,
    /// Authenticated user who created the session (M2).
    pub owner: Option<String>,
    pub vnc: bool,
    pub devtools: bool,
    /// A `{log_output_dir}/{id}.log` file exists right now.
    pub logs: bool,
}

// Admin-only: exposes per-session `owner`, the user↔session mapping
// that is deliberately kept off /status (see session/mod.rs).
pub async fn state(_admin: RequireAdmin, State(state): State<AppState>) -> Json<StateResponse> {
    let mut browsers: BTreeMap<String, BrowserInfo> = BTreeMap::new();
    let cfg = state.config();
    for (name, versions) in cfg.snapshot() {
        let mut keys: Vec<String> = versions.versions.keys().cloned().collect();
        keys.sort();
        browsers.insert(
            name.clone(),
            BrowserInfo {
                default: versions.default.clone(),
                versions: keys,
            },
        );
    }

    let mut sessions = Vec::with_capacity(state.sessions.len());
    state.sessions.each(|s| {
        sessions.push(SessionInfo {
            id: s.id().to_string(),
            browser: s.browser_name().to_string(),
            version: s.browser_version().to_string(),
            started: humantime::format_rfc3339(s.started()).to_string(),
            owner: s.owner().map(|o| o.to_string()),
            vnc: s.host_ports().vnc.is_some(),
            devtools: s.host_ports().devtools.is_some(),
            logs: false, // filled in below (each() is sync)
        });
    });
    let log_dir = PathBuf::from(&state.args.log_output_dir);
    for s in &mut sessions {
        s.logs = tokio::fs::metadata(log_dir.join(format!("{}.log", s.id)))
            .await
            .is_ok();
    }

    Json(StateResponse {
        capacity: Capacity {
            total: state.queue.capacity(),
            used: state.queue.used(),
            queued: state.queue.queued(),
            pending: state.queue.pending(),
        },
        draining: state.draining.load(Ordering::Relaxed),
        browsers,
        sessions,
    })
}
