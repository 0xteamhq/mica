//! Router-side POST /wd/hub/session — weighted placement + failover.
//!
//! 1. Buffer the body once; parse only browserName/browserVersion
//!    (`caps::Caps::parse`) for capability matching.
//! 2. Eligible nodes = Healthy ∧ weight>0 ∧ cached /status advertises
//!    the browser (health.rs::eligible_nodes).
//! 3. Weighted random WITHOUT replacement, up to
//!    `--router-max-attempts` attempts. The body is forwarded
//!    VERBATIM (no re-serialization).
//! 4. Failover on connect error / timeout / 5xx / 429; any other 4xx
//!    is the client's problem and returns unchanged.
//! 5. On success, rewrite `value.sessionId` (+ legacy `sessionId`,
//!    plus `value.capabilities.webSocketUrl` when present) to the
//!    routed id so every follow-up request self-describes its node.
//!
//! NO router-side queue: nodes already queue (`--limit` + counters),
//! and the long per-attempt timeout rides that out. A router queue
//! would need cross-replica coordination — the stateless tier stays
//! stateless. `X-Mica-No-Wait` passes through for fast-fail clients.

use super::registry::NodeConfig;
use super::{RouterState, health, session_id};
use crate::caps::Caps;
use crate::error::WdError;
use crate::observability::names::{ROUTER_CREATES_TOTAL, ROUTER_FAILOVERS_TOTAL};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::Value;

/// 122 random bits from a v4 UUID — enough entropy for weighted
/// selection with zero new dependencies. Swap for `rand` if placement
/// ever needs reproducible seeding.
fn random_below(n: u128) -> u128 {
    debug_assert!(n > 0);
    u128::from_le_bytes(*uuid::Uuid::new_v4().as_bytes()) % n
}

/// Weighted random without replacement.
fn pick_order(mut nodes: Vec<NodeConfig>, max: usize) -> Vec<NodeConfig> {
    let mut order = Vec::with_capacity(max.min(nodes.len()));
    while !nodes.is_empty() && order.len() < max {
        let total: u128 = nodes.iter().map(|n| n.weight as u128).sum();
        let mut roll = random_below(total);
        let mut idx = 0;
        for (i, n) in nodes.iter().enumerate() {
            let w = n.weight as u128;
            if roll < w {
                idx = i;
                break;
            }
            roll -= w;
        }
        order.push(nodes.swap_remove(idx));
    }
    order
}

pub async fn create_session(
    State(state): State<RouterState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WdError> {
    let parsed: Value = serde_json::from_slice(&body)
        .map_err(|e| WdError::invalid_argument(format!("request body: {e}")))?;
    let caps = Caps::parse(&parsed).map_err(|e| WdError::invalid_argument(e.to_string()))?;
    let browser = caps.browser_name.clone().unwrap_or_default();
    let version = caps.browser_version.clone();

    let eligible: Vec<NodeConfig> =
        health::eligible_nodes(&state.registry, &browser, version.as_deref())
            .into_iter()
            .map(|(n, _)| n)
            .collect();
    if eligible.is_empty() {
        return Err(WdError::session_not_created(format!(
            "no healthy node supports {browser} {}",
            version.as_deref().unwrap_or("(default)")
        )));
    }

    let attempts = pick_order(eligible, state.args.router_max_attempts as usize);
    let mut errors: Vec<String> = Vec::new();

    for node in attempts {
        let mut req = state
            .http
            .post(format!("{}/wd/hub/session", node.endpoint))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone())
            .timeout(state.args.router_create_timeout);
        if let Some(auth) = node.auth_header() {
            req = req.header(reqwest::header::AUTHORIZATION, auth);
        }
        for h in ["x-mica-no-wait", "x-request-id"] {
            if let Some(v) = headers.get(h) {
                req = req.header(h, v);
            }
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let mut json: Value = resp.json().await.map_err(|e| {
                        WdError::session_not_created(format!(
                            "node {}: parse create response: {e}",
                            node.name
                        ))
                    })?;
                    rewrite_response(&mut json, &node.name, &headers)?;
                    metrics::counter!(ROUTER_CREATES_TOTAL, "node" => node.name.clone())
                        .increment(1);
                    return Ok(Json(json).into_response());
                }
                let retryable = status.is_server_error() || status.as_u16() == 429;
                let text = resp.text().await.unwrap_or_default();
                if !retryable {
                    // Client-side error (bad caps, unauthorized …):
                    // retrying elsewhere would just repeat it. Return
                    // the node's status and W3C error body VERBATIM
                    // rather than masking every 4xx as a 500.
                    let code = axum::http::StatusCode::from_u16(status.as_u16())
                        .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
                    let mut out = Response::new(Body::from(text));
                    *out.status_mut() = code;
                    out.headers_mut().insert(
                        axum::http::header::CONTENT_TYPE,
                        axum::http::HeaderValue::from_static("application/json"),
                    );
                    return Ok(out);
                }
                errors.push(format!("{}: {status}: {text}", node.name));
            }
            Err(e) => errors.push(format!("{}: {e}", node.name)),
        }
        metrics::counter!(ROUTER_FAILOVERS_TOTAL).increment(1);
        tracing::warn!(node = %node.name, "create failed; trying next node");
    }

    Err(WdError::session_not_created(format!(
        "all nodes failed: [{}]",
        errors.join("; ")
    )))
}

/// Rewrite the upstream create response in place: session id gains
/// the node prefix; a BiDi webSocketUrl is repointed at the router
/// (best-effort — skipped when there's no Host header to build from).
fn rewrite_response(json: &mut Value, node_name: &str, headers: &HeaderMap) -> Result<(), WdError> {
    let upstream_id = json
        .pointer("/value/sessionId")
        .or_else(|| json.get("sessionId"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            WdError::session_not_created(format!("node {node_name}: response missing sessionId"))
        })?
        .to_string();
    let routed = session_id::encode(node_name, &upstream_id);
    tracing::info!(node = %node_name, session_id = %routed, "[SESSION_ROUTED]");

    if let Some(v) = json.pointer_mut("/value/sessionId") {
        *v = Value::String(routed.clone());
    }
    if let Some(v) = json.get_mut("sessionId") {
        *v = Value::String(routed.clone());
    }
    if let Some(ws) = json.pointer_mut("/value/capabilities/webSocketUrl")
        && let Some(host) = headers
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
    {
        // Match the scheme the client reached us on: a TLS-terminating
        // proxy in front of the router sets X-Forwarded-Proto=https, and
        // a plaintext ws:// URL would be rejected by mixed-content rules.
        let scheme = match headers
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
        {
            Some(p) if p.eq_ignore_ascii_case("https") => "wss",
            _ => "ws",
        };
        *ws = Value::String(format!("{scheme}://{host}/session/{routed}/bidi"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_zero_is_never_picked() {
        let nodes = vec![
            NodeConfig {
                name: "a".into(),
                endpoint: "http://a".into(),
                weight: 0,
                region: None,
                labels: Default::default(),
                username: None,
                password: None,
            },
            NodeConfig {
                name: "b".into(),
                endpoint: "http://b".into(),
                weight: 1,
                region: None,
                labels: Default::default(),
                username: None,
                password: None,
            },
        ];
        for _ in 0..50 {
            let order = pick_order(
                nodes.clone().into_iter().filter(|n| n.weight > 0).collect(),
                3,
            );
            assert!(order.iter().all(|n| n.name == "b"));
        }
    }

    #[test]
    fn pick_order_is_without_replacement() {
        let nodes: Vec<NodeConfig> = ["a", "b", "c"]
            .iter()
            .map(|n| NodeConfig {
                name: n.to_string(),
                endpoint: format!("http://{n}"),
                weight: 1,
                region: None,
                labels: Default::default(),
                username: None,
                password: None,
            })
            .collect();
        let order = pick_order(nodes, 3);
        let names: std::collections::HashSet<_> = order.iter().map(|n| n.name.clone()).collect();
        assert_eq!(names.len(), 3, "no node picked twice");
    }
}
