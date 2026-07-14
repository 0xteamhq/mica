//! Background node health poller.
//!
//! Every `--router-health-interval` the router GETs each node's
//! /status (with node credentials) and updates the registry's dynamic
//! state. State machine:
//!
//!   Unknown ──ok──▶ Healthy ◀──ok── Unhealthy
//!      │                │               ▲
//!      └──fail×N────────┴──fail×N───────┘
//!   Healthy ──ok+draining──▶ Draining (and back)
//!
//! Unhealthy/Draining/Unknown nodes are excluded from NEW-session
//! placement only; existing-session proxying still attempts the node
//! so a /status blip never severs live sessions. That asymmetry IS
//! the circuit breaker: it guards placement, not traffic.

use super::RouterState;
use super::registry::{NodeState, Registry, StatusSnapshot};
use crate::observability::names::{ROUTER_HEALTH_POLL_FAILURES_TOTAL, ROUTER_NODE_UP};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// One poll sweep over every configured node. Public so tests drive
/// state transitions deterministically instead of sleeping.
pub async fn poll_once(
    registry: &Registry,
    http: &reqwest::Client,
    timeout: Duration,
    unhealthy_threshold: u32,
) {
    let nodes = registry.config().nodes.clone();
    let polls = nodes.iter().map(|node| {
        let http = http.clone();
        async move {
            let mut req = http
                .get(format!("{}/status", node.endpoint))
                .timeout(timeout);
            if let Some(auth) = node.auth_header() {
                req = req.header(reqwest::header::AUTHORIZATION, auth);
            }
            let result: Result<StatusSnapshot, String> = async {
                let resp = req.send().await.map_err(|e| e.to_string())?;
                if !resp.status().is_success() {
                    return Err(format!("status {}", resp.status()));
                }
                resp.json::<StatusSnapshot>()
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            (node.name.clone(), result)
        }
    });

    for (name, result) in futures::future::join_all(polls).await {
        let mut d = registry.dynamic(&name).unwrap_or_default();
        match result {
            Ok(snapshot) => {
                d.state = if snapshot.draining {
                    NodeState::Draining
                } else {
                    NodeState::Healthy
                };
                d.consecutive_failures = 0;
                d.last_seen = Some(SystemTime::now());
                d.last_error = None;
                d.snapshot = Some(snapshot);
            }
            Err(e) => {
                d.consecutive_failures += 1;
                d.last_error = Some(e);
                metrics::counter!(ROUTER_HEALTH_POLL_FAILURES_TOTAL, "node" => name.clone())
                    .increment(1);
                if d.consecutive_failures >= unhealthy_threshold {
                    d.state = NodeState::Unhealthy;
                }
                // Below the threshold the previous state stands — one
                // blip must not flap placement.
            }
        }
        metrics::gauge!(ROUTER_NODE_UP, "node" => name.clone()).set(
            if d.state == NodeState::Healthy {
                1.0
            } else {
                0.0
            },
        );
        registry.set_dynamic(&name, d);
    }
}

/// Spawn the poll loop. Aborts when the returned handle is dropped by
/// serve.rs at shutdown.
pub fn spawn_poller(state: RouterState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = state.args.router_health_interval;
        let timeout = state.args.router_health_timeout;
        let threshold = state.args.router_unhealthy_threshold;
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            poll_once(&state.registry, &state.http, timeout, threshold).await;
        }
    })
}

/// Nodes eligible for new-session placement: Healthy AND weight > 0
/// AND the cached snapshot advertises the requested browser/version.
pub fn eligible_nodes(
    registry: &Registry,
    browser: &str,
    version: Option<&str>,
) -> Vec<(super::registry::NodeConfig, Arc<StatusSnapshot>)> {
    registry
        .config()
        .nodes
        .iter()
        .filter(|n| n.weight > 0)
        .filter_map(|n| {
            let d = registry.dynamic(&n.name)?;
            if d.state != NodeState::Healthy {
                return None;
            }
            let snapshot = d.snapshot?;
            if snapshot.supports(browser, version) {
                Some((n.clone(), Arc::new(snapshot)))
            } else {
                None
            }
        })
        .collect()
}
