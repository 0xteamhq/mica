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

    // `POST /wd/hub/session/{id}/file` (and Selenium-4 alias
    // `/se/file`) goes to the container's fileserver port, not to
    // the upstream WebDriver. Gated by `--enable-file-upload`.
    if state.args.enable_file_upload
        && method == Method::POST
        && matches!(tail.as_deref(), Some("file") | Some("se/file"))
    {
        let port = session
            .host_ports()
            .fileserver
            .clone()
            .ok_or_else(|| WdError::unknown_error("session has no fileserver port mapped"))?;
        return forward_to_fileserver(&state, &port, headers, body).await;
    }

    let upstream = session.upstream();
    let url = match tail.as_deref() {
        Some(t) if !t.is_empty() => format!("{upstream}/session/{session_id}/{t}"),
        _ => format!("{upstream}/session/{session_id}"),
    };

    // Build a WIT-shaped request snapshot for the plugin chain. We
    // strip hop-by-hop headers up-front so plugins see the same
    // canonical view as the upstream call.
    let mut wit_headers: Vec<crate::plugins::PluginHeader> = headers
        .iter()
        .filter_map(|(name, value)| {
            let n = name.as_str();
            if matches!(
                n,
                "host" | "connection" | "content-length" | "transfer-encoding"
            ) {
                return None;
            }
            value.to_str().ok().map(|v| crate::plugins::PluginHeader {
                name: n.to_string(),
                value: v.to_string(),
            })
        })
        .collect();
    let wit_req = crate::plugins::PluginHttpRequest {
        method: method.as_str().to_string(),
        path: url.clone(),
        headers: wit_headers.split_off(0),
        body: body.to_vec(),
    };

    // intercept-request plugin chain. Skipped when no plugin host
    // (zero overhead on the proxy hot path for installs without
    // plugins).
    let (final_req, short_resp) = if let Some(host) = state.plugins.as_ref() {
        match host
            .intercept_request(wit_req, state.args.plugin_http_timeout)
            .await
        {
            crate::plugins::RequestVerdict::Forward(req) => (req, None),
            crate::plugins::RequestVerdict::Short(resp) => (
                crate::plugins::PluginHttpRequest {
                    method: method.as_str().to_string(),
                    path: url.clone(),
                    headers: vec![],
                    body: body.to_vec(),
                },
                Some(resp),
            ),
        }
    } else {
        (wit_req, None)
    };

    if let Some(resp) = short_resp {
        return build_short_circuit_response(resp);
    }

    // Forward the (possibly modified) request upstream.
    let reqwest_method = reqwest::Method::from_bytes(final_req.method.as_bytes())
        .map_err(|e| WdError::unknown_error(format!("bad method: {e}")))?;
    let mut req = state.http.request(reqwest_method, &final_req.path);
    for h in &final_req.headers {
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(h.value.as_bytes())
            && let Ok(name) = reqwest::header::HeaderName::from_bytes(h.name.as_bytes())
        {
            req = req.header(name, v);
        }
    }
    if !final_req.body.is_empty() {
        req = req.body(final_req.body.clone());
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

    // intercept-response chain — plugins can rewrite the body /
    // headers before mica returns it to the client.
    let final_resp = if let Some(host) = state.plugins.as_ref() {
        let mut resp_headers: Vec<crate::plugins::PluginHeader> = upstream_headers
            .iter()
            .filter_map(|(k, v)| {
                let n = k.as_str();
                if matches!(n, "transfer-encoding" | "content-length" | "connection") {
                    return None;
                }
                v.to_str().ok().map(|s| crate::plugins::PluginHeader {
                    name: n.to_string(),
                    value: s.to_string(),
                })
            })
            .collect();
        let pr = crate::plugins::PluginHttpResponse {
            status: status.as_u16(),
            headers: resp_headers.split_off(0),
            body: bytes.to_vec(),
        };
        host.intercept_response(&final_req, pr, state.args.plugin_http_timeout)
            .await
    } else {
        crate::plugins::PluginHttpResponse {
            status: status.as_u16(),
            headers: upstream_headers
                .iter()
                .filter_map(|(k, v)| {
                    let n = k.as_str();
                    if matches!(n, "transfer-encoding" | "content-length" | "connection") {
                        return None;
                    }
                    v.to_str().ok().map(|s| crate::plugins::PluginHeader {
                        name: n.to_string(),
                        value: s.to_string(),
                    })
                })
                .collect(),
            body: bytes.to_vec(),
        }
    };
    let bytes = axum::body::Bytes::from(final_resp.body);
    let status = reqwest::StatusCode::from_u16(final_resp.status)
        .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    let mut upstream_headers = reqwest::header::HeaderMap::new();
    for h in final_resp.headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(h.name.as_bytes()),
            reqwest::header::HeaderValue::from_bytes(h.value.as_bytes()),
        ) {
            upstream_headers.insert(name, value);
        }
    }

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

/// Build an axum `Response` from a plugin-supplied
/// `PluginHttpResponse`. Used when an `intercept-request` plugin
/// returns `short-circuit` to skip the upstream call entirely.
fn build_short_circuit_response(
    resp: crate::plugins::PluginHttpResponse,
) -> Result<Response, WdError> {
    let mut builder = Response::builder().status(
        axum::http::StatusCode::from_u16(resp.status).unwrap_or(axum::http::StatusCode::OK),
    );
    for h in resp.headers {
        if matches!(
            h.name.as_str(),
            "transfer-encoding" | "content-length" | "connection"
        ) {
            continue;
        }
        if let Ok(name) = axum::http::HeaderName::from_bytes(h.name.as_bytes())
            && let Ok(value) = axum::http::HeaderValue::from_bytes(h.value.as_bytes())
        {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from(resp.body))
        .map_err(|e| WdError::unknown_error(format!("build short-circuit response: {e}")))
}

/// File-upload forwarder: takes the body from
/// `POST /wd/hub/session/{id}/file`, hands it to the container's
/// fileserver port at `/file`.
async fn forward_to_fileserver(
    state: &AppState,
    port: &str,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let url = format!("http://127.0.0.1:{port}/file");
    let mut req = state.http.post(&url);
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
        .map_err(|e| WdError::unknown_error(format!("file upload: {e}")))?;
    let status = resp.status();
    let upstream_headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| WdError::unknown_error(format!("file upload body: {e}")))?;
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
