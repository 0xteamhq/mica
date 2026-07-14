//! Router-mode entrypoint. `main.rs` branches here BEFORE isolation
//! probing and backend construction — a router never touches
//! docker.sock, kubeconfig, or wasmtime.

use super::registry::{NodesConfig, Registry};
use super::{RouterState, health};
use crate::auth::{AuthState, AuthSwap, require_basic_auth};
use crate::cli::Args;
use crate::{observability, shutdown};
use arc_swap::ArcSwap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};

static STARTED: OnceLock<SystemTime> = OnceLock::new();

pub fn uptime() -> Duration {
    STARTED
        .get()
        .and_then(|s| s.elapsed().ok())
        .unwrap_or_default()
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    STARTED.get_or_init(SystemTime::now);

    // Unlike browsers.json (warn-and-continue), a router without a
    // node registry serves nothing: hard error, same posture as a bad
    // --users file.
    let nodes = NodesConfig::load(&args.nodes)
        .map_err(|e| anyhow::anyhow!("--router requires a valid --nodes file: {e}"))?;
    tracing::info!(path = %args.nodes, nodes = nodes.nodes.len(), "router mode: node registry loaded");
    let registry = Arc::new(Registry::new(nodes));

    let auth: AuthSwap = match AuthState::load(&args.users) {
        Ok(s) => Arc::new(ArcSwap::from_pointee(s)),
        Err(e) => anyhow::bail!("read --users {}: {e}", args.users),
    };
    if !args.users.is_empty() {
        tracing::info!(path = %args.users, "HTTP Basic auth enabled (router)");
    }

    let prom = observability::install();
    let state = RouterState {
        registry: registry.clone(),
        args: Arc::new(args.clone()),
        // No global timeout: create/proxy set per-request budgets and
        // artifact downloads may stream for a while.
        http: reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .expect("build reqwest client"),
        metrics: Some(prom),
    };

    // SIGHUP reloads nodes.json + the users file. Parse failures keep
    // the previous state (the startup load already vetted both).
    #[cfg(unix)]
    {
        let nodes_path = args.nodes.clone();
        let users_path = args.users.clone();
        let registry = registry.clone();
        let auth_for_reload = auth.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let mut hup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "SIGHUP handler unavailable");
                    return;
                }
            };
            while hup.recv().await.is_some() {
                match NodesConfig::load(&nodes_path) {
                    Ok(c) => {
                        tracing::info!(path = %nodes_path, nodes = c.nodes.len(), "nodes.json reloaded");
                        registry.apply(c);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "nodes reload failed; keeping previous registry")
                    }
                }
                if !users_path.is_empty() {
                    match AuthState::load(&users_path) {
                        Ok(s) => auth_for_reload.store(Arc::new(s)),
                        Err(e) => tracing::warn!(error = %e, "users reload failed"),
                    }
                }
            }
        });
    }

    // Seed health state immediately so the first create doesn't race
    // an empty snapshot cache, then poll on the interval.
    health::poll_once(
        &state.registry,
        &state.http,
        args.router_health_timeout,
        args.router_unhealthy_threshold,
    )
    .await;
    let poller = health::spawn_poller(state.clone());

    let trace = TraceLayer::new_for_http()
        .make_span_with(
            DefaultMakeSpan::new()
                .level(tracing::Level::INFO)
                .include_headers(false),
        )
        .on_request(DefaultOnRequest::new().level(tracing::Level::DEBUG))
        .on_response(DefaultOnResponse::new().level(tracing::Level::DEBUG));

    let app = super::router(state)
        .layer(axum::middleware::from_fn_with_state(
            auth,
            require_basic_auth,
        ))
        .layer(trace);

    let listen = if args.listen.starts_with(':') {
        format!("0.0.0.0{}", args.listen)
    } else {
        args.listen.clone()
    };
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(addr = %listen, "mica router listening");

    // Graceful shutdown: no sessions to drain router-side, but
    // long-lived WS bridges (VNC/BiDi) would otherwise pin the
    // process forever — bound the wait with --graceful-period.
    let graceful = args.graceful_period;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown::signal_future().await;
            tracing::info!(?graceful, "router shutting down");
            tokio::spawn(async move {
                tokio::time::sleep(graceful).await;
                tracing::warn!("graceful period elapsed with connections still open; exiting");
                std::process::exit(0);
            });
        })
        .await?;
    poller.abort();
    Ok(())
}
