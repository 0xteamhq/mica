//! Router-side WebSocket relays: /vnc/{id} and /session/{id}/bidi.
//!
//! Pure WS↔WS: the node terminates its own bridges (WS→TCP for VNC,
//! WS→WS for BiDi), so the router only ever speaks WebSocket to the
//! node. Frame pump shared with the node via `crate::ws_bridge`, with
//! the node's Basic credentials on the upgrade handshake.
//!
//! Caveat (documented risk): these are long-lived connections. Router
//! shutdown severs them after `--graceful-period`; WebDriver HTTP
//! traffic is unaffected because any replica can route any session.

use super::{RouterState, session_id};
use crate::error::WdError;
use crate::ws_bridge;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::response::Response;
use tokio_tungstenite::tungstenite::http::HeaderValue;

fn ws_target(endpoint: &str, path: String) -> Option<String> {
    endpoint
        .strip_prefix("http://")
        .map(|tail| format!("ws://{tail}{path}"))
        .or_else(|| {
            endpoint
                .strip_prefix("https://")
                .map(|tail| format!("wss://{tail}{path}"))
        })
}

fn upgrade_to(
    state: &RouterState,
    routed_id: &str,
    path_for: impl FnOnce(&str) -> String,
    ws: WebSocketUpgrade,
) -> Result<Response, WdError> {
    let (node_name, upstream_id) = session_id::decode(routed_id)
        .ok_or_else(|| WdError::invalid_session_id(format!("unknown session: {routed_id}")))?;
    let node = state.registry.node(&node_name).ok_or_else(|| {
        WdError::invalid_session_id(format!("session {routed_id}: node no longer registered"))
    })?;
    let target = ws_target(&node.endpoint, path_for(&upstream_id))
        .ok_or_else(|| WdError::unknown_error("node endpoint is not http(s)"))?;
    let auth = node
        .auth_header()
        .and_then(|a| HeaderValue::from_str(&a).ok());
    Ok(ws.on_upgrade(move |sock| ws_bridge::bridge(sock, target, auth)))
}

pub async fn vnc(
    State(state): State<RouterState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, WdError> {
    upgrade_to(&state, &session_id, |id| format!("/vnc/{id}"), ws)
}

pub async fn bidi(
    State(state): State<RouterState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, WdError> {
    upgrade_to(&state, &session_id, |id| format!("/session/{id}/bidi"), ws)
}
