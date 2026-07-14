//! Admin control plane — `/admin` (embedded SPA) + `/admin/api/*`.
//!
//! M1 ships the read-only surface: a full dashboard snapshot and an
//! SSE stream of lifecycle events. Both sit behind the same Basic
//! auth gate as the rest of the API (`/admin*` is NOT in the auth
//! open-path list).
//!
//! Wire contract (`/admin/api/*` JSON shapes) is internal to the
//! bundled UI — it may change between minor versions, unlike /status.

pub mod assets;
pub mod events;
pub mod ops;
pub mod quotas;
pub mod registry;
pub mod state;
pub mod users;

use crate::state::AppState;
use axum::Router;
use axum::routing::{delete, get, post, put};

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/state", get(state::state))
        .route("/events", get(events::events))
        .route("/sessions/:session_id", delete(ops::kill_session))
        .route("/drain", post(ops::drain))
        .route("/config/reload", post(ops::config_reload))
        .route(
            "/config/browsers",
            get(registry::get_browsers).put(registry::put_browsers),
        )
        .route("/users", get(users::list))
        .route("/users/:name", put(users::upsert).delete(users::delete))
        .route("/quotas", get(quotas::get).put(quotas::put))
}
