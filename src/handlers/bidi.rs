//! GET /session/{session_id}/bidi — WebSocket multiplex for the W3C
//! BiDi protocol (P6.1).
//!
//! Selenium 4 / Playwright drive sessions over a single bidirectional
//! WebSocket instead of HTTP polling. mica accepts the inbound upgrade
//! and bridges it to the upstream browser's BiDi WebSocket port,
//! preserving frame boundaries and Close codes.

use crate::error::WdError;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TMessage;

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

    Ok(ws.on_upgrade(move |sock| bridge(sock, target)))
}

async fn bridge(socket: WebSocket, target: String) {
    let (upstream_stream, _resp) = match connect_async(&target).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target = %target, error = %e, "bidi upstream connect failed");
            return;
        }
    };
    let (mut up_tx, mut up_rx) = upstream_stream.split();
    let (mut ws_tx, mut ws_rx) = socket.split();

    let to_upstream = async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            let out = match msg {
                Message::Text(t) => TMessage::Text(t),
                Message::Binary(b) => TMessage::Binary(b),
                Message::Ping(p) => TMessage::Ping(p),
                Message::Pong(p) => TMessage::Pong(p),
                Message::Close(c) => TMessage::Close(c.map(|cf| {
                    tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: cf.code.into(),
                        reason: cf.reason,
                    }
                })),
            };
            if up_tx.send(out).await.is_err() {
                break;
            }
        }
        let _ = up_tx.close().await;
    };

    let to_client = async move {
        while let Some(Ok(msg)) = up_rx.next().await {
            let out = match msg {
                TMessage::Text(t) => Message::Text(t),
                TMessage::Binary(b) => Message::Binary(b),
                TMessage::Ping(p) => Message::Ping(p),
                TMessage::Pong(p) => Message::Pong(p),
                TMessage::Close(c) => Message::Close(c.map(|cf| axum::extract::ws::CloseFrame {
                    code: cf.code.into(),
                    reason: cf.reason,
                })),
                TMessage::Frame(_) => continue,
            };
            if ws_tx.send(out).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };

    tokio::join!(to_upstream, to_client);
}
