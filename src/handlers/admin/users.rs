//! User management (M3) — mica becomes the editor of record for the
//! htpasswd file (format v2, `name:hash[:admin]`; see src/auth.rs).
//!
//!   GET    /admin/api/users        — [{"name", "admin"}] (no hashes)
//!   PUT    /admin/api/users/:name  — {"password"?, "admin"?} upsert
//!   DELETE /admin/api/users/:name
//!
//! Every write persists to `--users` first (tmp + rename), then swaps
//! the live AuthState so the Basic gate picks it up immediately.
//! Requires `--users` to be configured — in open mode there is no
//! file to edit (409).

use crate::auth::RequireAdmin;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::sync::Arc;

pub async fn list(_admin: RequireAdmin, State(state): State<AppState>) -> Json<serde_json::Value> {
    let users: Vec<_> = state
        .auth
        .load()
        .entries()
        .into_iter()
        .map(|(name, admin)| serde_json::json!({ "name": name, "admin": admin }))
        .collect();
    Json(serde_json::Value::Array(users))
}

#[derive(Deserialize)]
pub struct UpsertRequest {
    /// Plaintext password, bcrypt-hashed server-side. Optional when
    /// updating an existing user's role only.
    pub password: Option<String>,
    #[serde(default)]
    pub admin: bool,
}

pub async fn upsert(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpsertRequest>,
) -> Response {
    if state.args.users.is_empty() {
        return no_users_file();
    }
    if name.is_empty() || name.contains(':') || name.contains('\n') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid user name" })),
        )
            .into_response();
    }

    let _write = state.file_write_lock.lock().await;
    let mut next = (*state.auth.load_full()).clone();
    match req.password.as_deref() {
        Some(pw) if !pw.is_empty() => {
            let hash = match bcrypt::hash(pw, bcrypt::DEFAULT_COST) {
                Ok(h) => h,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("bcrypt: {e}") })),
                    )
                        .into_response();
                }
            };
            next.upsert(&name, hash, req.admin);
        }
        _ => {
            // Role-only update needs an existing row to keep its hash.
            if !next.set_admin(&name, req.admin) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "password required for a new user" })),
                )
                    .into_response();
            }
        }
    }
    persist_and_swap(&state, next).await
}

pub async fn delete(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    if state.args.users.is_empty() {
        return no_users_file();
    }
    let _write = state.file_write_lock.lock().await;
    let mut next = (*state.auth.load_full()).clone();
    if !next.remove(&name) {
        return (StatusCode::NOT_FOUND, "unknown user").into_response();
    }
    persist_and_swap(&state, next).await
}

async fn persist_and_swap(state: &AppState, next: crate::auth::AuthState) -> Response {
    if let Err(e) =
        super::registry::write_atomic(&state.args.users, next.to_file_string().as_bytes()).await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write {}: {e}", state.args.users) })),
        )
            .into_response();
    }
    state.auth.store(Arc::new(next));
    tracing::info!(path = %state.args.users, "users file updated via admin API");
    StatusCode::NO_CONTENT.into_response()
}

fn no_users_file() -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "no users file configured (--users); auth is open"
        })),
    )
        .into_response()
}
