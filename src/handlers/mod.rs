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
use axum::routing::{any, delete, get, post};

pub fn router(state: AppState) -> Router {
    Router::new()
        // M1 + M9 T37
        .route("/ping", get(ping::ping_with_state))
        // M9 T36
        .route("/status", get(status::status))
        // M8
        .route("/wd/hub/session", post(create::create_session))
        .route("/wd/hub/session/:session_id", any(proxy::proxy_with_id))
        .route(
            "/wd/hub/session/:session_id/*tail",
            any(proxy::proxy_with_tail),
        )
        // M9 T38 — VNC websocket bridge
        .route("/vnc/:session_id", get(vnc::vnc))
        // M9 T39 — video file server
        .route("/video/:name", get(artifacts::get_video))
        .route("/video/:name", delete(artifacts::delete_video))
        // M9 T40 — log file server
        .route("/logs/:name", get(artifacts::get_log))
        .route("/logs/:name", delete(artifacts::delete_log))
        // M9 T41 — relay group
        .route("/devtools/:session_id", any(relay::devtools_root))
        .route("/devtools/:session_id/*tail", any(relay::devtools))
        .route("/clipboard/:session_id", any(relay::clipboard))
        .route("/download/:session_id", any(relay::download_root))
        .route("/download/:session_id/*tail", any(relay::download))
        .with_state(state)
}
