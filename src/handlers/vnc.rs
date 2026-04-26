//! GET /vnc/{session_id} — WebSocket → container VNC TCP socket.
//!
//! T38: bridge an inbound WebSocket to the host port mica allocated
//! for the container's VNC port (5900). Two `tokio::io::copy_bidirectional`
//! halves run after the upgrade.

use crate::error::WdError;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn vnc(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, WdError> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| WdError::invalid_session_id(format!("unknown session: {session_id}")))?;
    let port = session
        .host_ports()
        .vnc
        .clone()
        .ok_or_else(|| WdError::unknown_error("session has no VNC port mapped"))?;
    Ok(ws.on_upgrade(move |sock| bridge(sock, port)))
}

async fn bridge(socket: WebSocket, port: String) {
    let addr = format!("127.0.0.1:{port}");
    let tcp = match tokio::net::TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target = %addr, error = %e, "vnc tcp connect failed");
            return;
        }
    };
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();

    // ws -> tcp
    let to_tcp = async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Binary(b) => {
                    if tcp_tx.write_all(&b).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = tcp_tx.shutdown().await;
    };

    // tcp -> ws
    let to_ws = async move {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_tx
                        .send(Message::Binary(buf[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };

    tokio::join!(to_tcp, to_ws);
}
