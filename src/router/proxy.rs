//! Router-side HTTP forwarding for session-scoped endpoints.
//!
//! Every handler decodes the node prefix from the routed session id
//! (session_id.rs), resolves the node in the registry, and forwards
//! the request with the BARE upstream id — the node's SessionMap is
//! keyed by the id its own browser generated.
//!
//! Responses are STREAMED (`Body::from_stream`), unlike the node's
//! buffering proxy — the router must not hold a screenshot or video
//! payload in memory twice.
//!
//! The client's Authorization header is stripped (auth terminates at
//! the router); per-node credentials are injected instead. Artifacts
//! are proxied, never redirected: node endpoints are typically not
//! client-reachable, and a redirect would leak internal topology.
//!
//! Unhealthy nodes are still attempted here — health only gates NEW
//! session placement. If the node is truly down the forward fails on
//! its own.

use super::{RouterState, session_id};
use crate::error::WdError;
use crate::observability::names::ROUTER_PROXY_REQUESTS_TOTAL;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;

/// Decode a routed id and resolve its node, or the W3C error that
/// makes a stale/foreign id look exactly like an expired session.
fn resolve(
    state: &RouterState,
    routed_id: &str,
) -> Result<(super::registry::NodeConfig, String), WdError> {
    let (node_name, upstream_id) = session_id::decode(routed_id)
        .ok_or_else(|| WdError::invalid_session_id(format!("unknown session: {routed_id}")))?;
    let node = state.registry.node(&node_name).ok_or_else(|| {
        WdError::invalid_session_id(format!("session {routed_id}: node no longer registered"))
    })?;
    Ok((node, upstream_id))
}

async fn forward(
    state: &RouterState,
    node: &super::registry::NodeConfig,
    path_and_query: String,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    metrics::counter!(ROUTER_PROXY_REQUESTS_TOTAL, "node" => node.name.clone()).increment(1);
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|e| WdError::unknown_error(format!("bad method: {e}")))?;
    let mut req = state
        .http
        .request(method, format!("{}{}", node.endpoint, path_and_query));
    for (name, value) in headers.iter() {
        // Hop-by-hop + host + the client's credentials stay here.
        if matches!(
            name.as_str(),
            "host" | "connection" | "content-length" | "transfer-encoding" | "authorization"
        ) {
            continue;
        }
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
            && let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes())
        {
            req = req.header(n, v);
        }
    }
    if let Some(auth) = node.auth_header() {
        req = req.header(reqwest::header::AUTHORIZATION, auth);
    }
    if !body.is_empty() {
        req = req.body(body);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| WdError::unknown_error(format!("node {}: {e}", node.name)))?;

    let mut builder = Response::builder().status(
        axum::http::StatusCode::from_u16(resp.status().as_u16())
            .unwrap_or(axum::http::StatusCode::BAD_GATEWAY),
    );
    for (name, value) in resp.headers() {
        if matches!(
            name.as_str(),
            "transfer-encoding" | "content-length" | "connection"
        ) {
            continue;
        }
        if let Ok(n) = axum::http::HeaderName::from_bytes(name.as_str().as_bytes())
            && let Ok(v) = axum::http::HeaderValue::from_bytes(value.as_bytes())
        {
            builder = builder.header(n, v);
        }
    }
    builder
        .body(Body::from_stream(resp.bytes_stream()))
        .map_err(|e| WdError::unknown_error(format!("build response: {e}")))
}

/// Shared shape for the session-scoped handlers: rebuild the node
/// path with the bare id + optional tail.
async fn forward_session_scoped(
    state: RouterState,
    prefix: &str,
    routed_id: String,
    tail: Option<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let (node, upstream_id) = resolve(&state, &routed_id)?;
    let path = match tail.as_deref() {
        Some(t) if !t.is_empty() => format!("{prefix}/{upstream_id}/{t}"),
        _ => format!("{prefix}/{upstream_id}"),
    };
    forward(&state, &node, path, method, headers, body).await
}

macro_rules! session_scoped {
    ($root:ident, $tail_fn:ident, $prefix:literal) => {
        pub async fn $root(
            State(state): State<RouterState>,
            Path(session_id): Path<String>,
            method: Method,
            headers: HeaderMap,
            body: Bytes,
        ) -> Result<Response, WdError> {
            forward_session_scoped(state, $prefix, session_id, None, method, headers, body).await
        }

        pub async fn $tail_fn(
            State(state): State<RouterState>,
            Path((session_id, tail)): Path<(String, String)>,
            method: Method,
            headers: HeaderMap,
            body: Bytes,
        ) -> Result<Response, WdError> {
            forward_session_scoped(
                state,
                $prefix,
                session_id,
                Some(tail),
                method,
                headers,
                body,
            )
            .await
        }
    };
}

session_scoped!(session, session_tail, "/wd/hub/session");
session_scoped!(devtools, devtools_tail, "/devtools");
session_scoped!(download, download_tail, "/download");

pub async fn clipboard(
    State(state): State<RouterState>,
    Path(session_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    forward_session_scoped(state, "/clipboard", session_id, None, method, headers, body).await
}

/// Artifact names are `{session_id}.{ext}` (see the node's cancel
/// hook) — in router mode the session id inside the name carries the
/// node prefix.
async fn forward_artifact(
    state: RouterState,
    kind: &str,
    name: String,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, WdError> {
    let (sid, ext) = name
        .rsplit_once('.')
        .ok_or_else(|| WdError::invalid_argument(format!("bad artifact name: {name}")))?;
    let (node, upstream_id) = resolve(&state, sid)?;
    let path = format!("/{kind}/{upstream_id}.{ext}");
    forward(&state, &node, path, method, headers, Bytes::new()).await
}

pub async fn video(
    State(state): State<RouterState>,
    Path(name): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, WdError> {
    forward_artifact(state, "video", name, method, headers).await
}

pub async fn logs(
    State(state): State<RouterState>,
    Path(name): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, WdError> {
    forward_artifact(state, "logs", name, method, headers).await
}
