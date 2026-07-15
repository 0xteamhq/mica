//! Admin control plane — `/admin` (embedded SPA) + `/admin/api/*`.
//!
//! M1 ships the read-only surface: a full dashboard snapshot and an
//! SSE stream of lifecycle events. `/admin*` is NOT in the auth
//! open-path list, and both reads additionally require the `admin`
//! role (`RequireAdmin`) because they expose per-session `owner` —
//! the user↔session mapping kept off /status (see session/mod.rs).
//!
//! Wire contract (`/admin/api/*` JSON shapes) is internal to the
//! bundled UI — it may change between minor versions, unlike /status.

pub mod assets;
pub mod events;
pub mod ops;
pub mod quotas;
pub mod recordings;
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
        .route("/recordings", get(recordings::list))
}
