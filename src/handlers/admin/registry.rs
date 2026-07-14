//! Browser-registry editing (M3).
//!
//!   GET /admin/api/config/browsers — raw browsers.json bytes
//!   PUT /admin/api/config/browsers — validate, persist, hot-swap
//!
//! The FILE stays the source of truth. `Browser` is Deserialize-only
//! and serde drops unknown fields, so we never round-trip through the
//! structs: GET returns the file verbatim and PUT persists the
//! client's exact bytes (after validation) via tmp-file + atomic
//! rename, then swaps the parsed config in. A concurrent SIGHUP
//! re-reads the same file, so both paths converge.

use crate::auth::RequireAdmin;
use crate::config::Config;
use crate::events::AdminEvent;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub async fn get_browsers(_admin: RequireAdmin, State(state): State<AppState>) -> Response {
    match tokio::fs::read(&state.args.conf).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("read {}: {e}", state.args.conf) })),
        )
            .into_response(),
    }
}

pub async fn put_browsers(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let config = match Config::validate_bytes(&body) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let _write = state.file_write_lock.lock().await;
    if let Err(e) = write_atomic(&state.args.conf, &body).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write {}: {e}", state.args.conf) })),
        )
            .into_response();
    }
    state.config_swap.store(Arc::new(config));
    state.events.emit_admin(AdminEvent::ConfigReloaded);
    tracing::info!(path = %state.args.conf, "browsers.json updated via admin API");
    StatusCode::NO_CONTENT.into_response()
}

/// Write via tmp file + rename — atomic on the same filesystem, so a
/// crash mid-write can't leave a torn file for SIGHUP to trip over.
pub(super) async fn write_atomic(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}
