//! Shared WebSocket↔WebSocket bridge.
//!
//! One implementation serves both consumers:
//!   - handlers/bidi.rs — node-side BiDi multiplex to the browser
//!   - router/proxy_ws.rs — router-side relay to a node's /vnc and
//!     /session/{id}/bidi endpoints (with node credentials)
//!
//! Frames are translated per-message (text/binary/ping/pong/close);
//! Close codes are preserved. Continuation-frame boundaries are not —
//! acceptable for BiDi/CDP/VNC traffic, which is message-oriented.

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;

/// Connect to `target` (ws:// or wss://) and pump frames both ways
/// until either side closes. `authorization`, when set, rides on the
/// upgrade handshake — the router uses it to present per-node Basic
/// credentials.
pub async fn bridge(socket: WebSocket, target: String, authorization: Option<HeaderValue>) {
    let mut request = match target.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target = %target, error = %e, "ws bridge: bad target");
            return;
        }
    };
    if let Some(auth) = authorization {
        request.headers_mut().insert(AUTHORIZATION, auth);
    }

    let (upstream_stream, _resp) = match connect_async(request).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target = %target, error = %e, "ws bridge: upstream connect failed");
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
