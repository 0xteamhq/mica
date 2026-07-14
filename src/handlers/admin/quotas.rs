//! Per-user quota config (M3).
//!
//!   GET /admin/api/quotas — current limits + per-user in-flight counts
//!   PUT /admin/api/quotas — replace limits; persisted to `--quotas`
//!                           when configured, in-memory otherwise
//!
//! Limits apply to NEW sessions only — lowering a quota never kills
//! running sessions, the user just can't create more until they drop
//! below the new limit.

use crate::auth::RequireAdmin;
use crate::quota::Quotas;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::BTreeMap;

pub async fn get(_admin: RequireAdmin, State(state): State<AppState>) -> Json<serde_json::Value> {
    let quotas = state.quotas.snapshot();
    let in_use: BTreeMap<_, _> = quotas
        .users
        .keys()
        .map(|u| (u.clone(), state.quotas.in_use(u)))
        .collect();
    Json(serde_json::json!({
        "default": quotas.default,
        "users": quotas.users,
        "inUse": in_use,
    }))
}

pub async fn put(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Json(quotas): Json<Quotas>,
) -> Response {
    // Persist first when a file backs the quotas, so SIGHUP reload
    // and this API stay convergent.
    if !state.args.quotas.is_empty() {
        let bytes = serde_json::to_vec_pretty(&quotas).expect("quotas serialize");
        let _write = state.file_write_lock.lock().await;
        if let Err(e) = super::registry::write_atomic(&state.args.quotas, &bytes).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("write {}: {e}", state.args.quotas) })),
            )
                .into_response();
        }
    }
    state.quotas.store(quotas);
    tracing::info!("quotas updated via admin API");
    StatusCode::NO_CONTENT.into_response()
}
