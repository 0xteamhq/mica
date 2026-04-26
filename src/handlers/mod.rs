//! HTTP handlers — entry points for the WebDriver wire protocol and
//! mica's auxiliary endpoints.
//!
//! Routing layout:
//!   GET  /ping                                  -> M1 ping
//!   POST /wd/hub/session                        -> M8 create
//!   GET/POST/PUT/DELETE /wd/hub/session/{id}/*  -> M8 proxy / delete
//!
//! M9 will add /status, /vnc/{id}, /video, /logs, and the relay group.

pub mod artifacts;
pub mod create;
pub mod ping;
pub mod proxy;
pub mod relay;
pub mod status;
pub mod vnc;

use crate::state::AppState;
use axum::Router;
use axum::routing::{any, get, post};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ping", get(ping::ping))
        .route("/wd/hub/session", post(create::create_session))
        .route("/wd/hub/session/:session_id", any(proxy::proxy_with_id))
        .route(
            "/wd/hub/session/:session_id/*tail",
            any(proxy::proxy_with_tail),
        )
        .with_state(state)
}
