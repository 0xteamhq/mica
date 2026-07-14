//! Mutating admin operations (M2). All routes take `RequireAdmin`:
//! open when auth is disabled, admin-role-only otherwise.
//!
//!   DELETE /admin/api/sessions/{id}   — kill a session
//!   POST   /admin/api/drain           — {"active": bool} set/clear
//!   POST   /admin/api/config/reload   — re-read browsers.json + users

use crate::auth::RequireAdmin;
use crate::events::AdminEvent;
use crate::reload;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::sync::atomic::Ordering;

pub async fn kill_session(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Response {
    // remove() fires the session's cancel hook: queue slot released,
    // container stopped, SessionStopped emitted. Idempotent with the
    // idle reaper — run_cancel only fires once.
    if state.sessions.remove(&session_id).await {
        tracing::info!(session_id = %session_id, "[SESSION_KILLED] via admin API");
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "unknown session").into_response()
    }
}

#[derive(Deserialize)]
pub struct DrainRequest {
    pub active: bool,
}

pub async fn drain(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Json(req): Json<DrainRequest>,
) -> Json<serde_json::Value> {
    let was = state.draining.swap(req.active, Ordering::Relaxed);
    if was != req.active {
        tracing::info!(active = req.active, "drain toggled via admin API");
        state
            .events
            .emit_admin(AdminEvent::Drain { active: req.active });
    }
    Json(serde_json::json!({
        "draining": req.active,
        "sessions": state.sessions.len(),
    }))
}

pub async fn config_reload(_admin: RequireAdmin, State(state): State<AppState>) -> Response {
    match reload::reload_all(&state) {
        Ok(summary) => Json(summary).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
