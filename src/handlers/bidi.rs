//! GET /session/{session_id}/bidi — WebSocket multiplex for the W3C
//! BiDi protocol (P6.1).
//!
//! Selenium 4 / Playwright drive sessions over a single bidirectional
//! WebSocket instead of HTTP polling. mica accepts the inbound upgrade
//! and bridges it to the upstream browser's BiDi WebSocket port,
//! preserving frame boundaries and Close codes. The frame pump lives
//! in `crate::ws_bridge` (shared with router mode).

use crate::error::WdError;
use crate::state::AppState;
use crate::ws_bridge;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::response::Response;

pub async fn bidi(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, WdError> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| WdError::invalid_session_id(format!("unknown session: {session_id}")))?;

    // BiDi conventionally runs on the same port as the WebDriver HTTP
    // upstream, scheme switched to ws://. Browsers that publish BiDi
    // on a different port surface the address through CDP; that case
    // is handled by browsers.json adding a `path` override that
    // resolves to the BiDi endpoint.
    let upstream = session.upstream().to_string();
    let target = upstream
        .strip_prefix("http://")
        .map(|tail| format!("ws://{tail}/session/{session_id}"))
        .or_else(|| {
            upstream
                .strip_prefix("https://")
                .map(|tail| format!("wss://{tail}/session/{session_id}"))
        })
        .ok_or_else(|| WdError::unknown_error("upstream is not http(s)"))?;

    Ok(ws.on_upgrade(move |sock| ws_bridge::bridge(sock, target, None)))
}
