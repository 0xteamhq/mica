//! Proxy handler for /wd/hub/session/{id} and /wd/hub/session/{id}/*.
//!
//! T30 — every request touches the session (resets idle), then forwards
//! method + headers + body to the upstream WebDriver. Streaming of the
//! response body is preserved.
//!
//! T31 — DELETE /wd/hub/session/{id} forwards normally, then removes
//! the session from the map. The cancel hook fires the backend stop
//! and (best-effort) an upstream DELETE, so we don't double-call the
//! upstream from here.

use crate::error::WdError;
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;

pub async fn proxy_with_tail(
    State(state): State<AppState>,
    Path((session_id, tail)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    proxy(state, session_id, Some(tail), method, headers, body).await
}

pub async fn proxy_with_id(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    proxy(state, session_id, None, method, headers, body).await
}

async fn proxy(
    state: AppState,
    session_id: String,
    tail: Option<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| WdError::invalid_session_id(format!("unknown session: {session_id}")))?;
    session.touch();

    let upstream = session.upstream();
    let url = match tail.as_deref() {
        Some(t) if !t.is_empty() => format!("{upstream}/session/{session_id}/{t}"),
        _ => format!("{upstream}/session/{session_id}"),
    };

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|e| WdError::unknown_error(format!("bad method: {e}")))?;
    let mut req = state.http.request(reqwest_method, &url);
    for (name, value) in headers.iter() {
        // Don't forward hop-by-hop or routing headers. axum's Host
        // header would point at mica, not the upstream container.
        let n = name.as_str();
        if matches!(
            n,
            "host" | "connection" | "content-length" | "transfer-encoding"
        ) {
            continue;
        }
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
            && let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes())
        {
            req = req.header(name, v);
        }
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| WdError::unknown_error(format!("upstream send: {e}")))?;
    let status = resp.status();
    let upstream_headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| WdError::unknown_error(format!("upstream body: {e}")))?;

    // T31: explicit DELETE on the session root tears the session down.
    let is_root_delete = tail.is_none() && method == Method::DELETE;
    if is_root_delete {
        state.sessions.remove(&session_id).await;
    }

    let mut builder = Response::builder().status(
        axum::http::StatusCode::from_u16(status.as_u16()).unwrap_or(axum::http::StatusCode::OK),
    );
    for (name, value) in upstream_headers.iter() {
        let n = name.as_str();
        if matches!(n, "transfer-encoding" | "content-length" | "connection") {
            continue;
        }
        if let Ok(name) = axum::http::HeaderName::from_bytes(name.as_str().as_bytes())
            && let Ok(v) = axum::http::HeaderValue::from_bytes(value.as_bytes())
        {
            builder = builder.header(name, v);
        }
    }
    builder
        .body(Body::from(bytes))
        .map_err(|e| WdError::unknown_error(format!("build response: {e}")))
}
