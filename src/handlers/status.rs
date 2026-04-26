//! GET /status — grid health snapshot.

use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct StatusResponse {
    pub total: usize,
    pub used: usize,
    pub queued: usize,
    pub pending: usize,
    pub browsers: BTreeMap<String, Vec<String>>,
    pub sessions: Vec<SessionEntry>,
}

#[derive(Serialize)]
pub struct SessionEntry {
    pub id: String,
    pub browser: String,
    pub version: String,
    pub started: String,
}

pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut browsers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let cfg = state.config();
    for (name, versions) in cfg.snapshot() {
        let mut keys: Vec<String> = versions.versions.keys().cloned().collect();
        keys.sort();
        browsers.insert(name.clone(), keys);
    }

    let mut sessions = Vec::with_capacity(state.sessions.len());
    state.sessions.each(|s| {
        sessions.push(SessionEntry {
            id: s.id().to_string(),
            browser: s.browser_name().to_string(),
            version: s.browser_version().to_string(),
            started: humantime::format_rfc3339(s.started()).to_string(),
        });
    });

    Json(StatusResponse {
        total: state.queue.capacity(),
        used: state.queue.used(),
        queued: state.queue.queued(),
        pending: state.queue.pending(),
        browsers,
        sessions,
    })
}
