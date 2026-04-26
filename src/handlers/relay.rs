//! Relay group — `/devtools/{session}/{*tail}`, `/clipboard/{session}`,
//! `/download/{session}/{*tail}`. Each is a thin reverse proxy onto a
//! specific port from the session's `HostPorts`.
//!
//! T41 — kept as a generic helper so the K8s and Firecracker backends
//! reuse it unchanged once they fill in the appropriate `HostPorts`.

use crate::error::WdError;
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;

#[derive(Clone, Copy)]
pub enum Service {
    DevTools,
    Clipboard,
    Download,
}

impl Service {
    fn port(self, p: &crate::backend::HostPorts) -> Option<&str> {
        match self {
            Service::DevTools => p.devtools.as_deref(),
            Service::Clipboard => p.clipboard.as_deref(),
            Service::Download => p.fileserver.as_deref(),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Service::DevTools => "devtools",
            Service::Clipboard => "clipboard",
            Service::Download => "download",
        }
    }
}

pub async fn devtools(
    state: State<AppState>,
    path: Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let (sid, tail) = path.0;
    relay(
        state.0,
        Service::DevTools,
        sid,
        Some(tail),
        method,
        headers,
        body,
    )
    .await
}

pub async fn devtools_root(
    state: State<AppState>,
    path: Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    relay(
        state.0,
        Service::DevTools,
        path.0,
        None,
        method,
        headers,
        body,
    )
    .await
}

pub async fn clipboard(
    state: State<AppState>,
    path: Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    relay(
        state.0,
        Service::Clipboard,
        path.0,
        None,
        method,
        headers,
        body,
    )
    .await
}

pub async fn download(
    state: State<AppState>,
    path: Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let (sid, tail) = path.0;
    relay(
        state.0,
        Service::Download,
        sid,
        Some(tail),
        method,
        headers,
        body,
    )
    .await
}

pub async fn download_root(
    state: State<AppState>,
    path: Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    relay(
        state.0,
        Service::Download,
        path.0,
        None,
        method,
        headers,
        body,
    )
    .await
}

async fn relay(
    state: AppState,
    svc: Service,
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
    let port = svc.port(session.host_ports()).ok_or_else(|| {
        WdError::unknown_error(format!("session has no {} port mapped", svc.label()))
    })?;
    let url = match tail.as_deref() {
        Some(t) if !t.is_empty() => format!("http://127.0.0.1:{port}/{t}"),
        _ => format!("http://127.0.0.1:{port}/"),
    };

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|e| WdError::unknown_error(format!("bad method: {e}")))?;
    let mut req = state.http.request(reqwest_method, &url);
    for (name, value) in headers.iter() {
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
        .map_err(|e| WdError::unknown_error(format!("relay {}: {e}", svc.label())))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| WdError::unknown_error(format!("relay body: {e}")))?;
    let mut builder = Response::builder().status(
        axum::http::StatusCode::from_u16(status.as_u16()).unwrap_or(axum::http::StatusCode::OK),
    );
    for (name, value) in headers.iter() {
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
