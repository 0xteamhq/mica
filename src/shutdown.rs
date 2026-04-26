//! Graceful shutdown helpers.
//!
//! T48 — `signal_future` resolves on SIGTERM or Ctrl-C (Unix) /
//! Ctrl-C (other platforms). `drain` walks `SessionMap` and removes
//! every active session (which fires each session's cancel hook,
//! tearing down the container). The serve loop wraps both via
//! `axum::serve(...).with_graceful_shutdown(signal_future)`.

use crate::session::SessionMap;
use std::time::Duration;

/// Future that resolves once a shutdown signal is received.
pub async fn signal_future() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        tokio::select! {
            _ = term.recv() => tracing::info!("received SIGTERM, draining"),
            _ = int.recv() => tracing::info!("received SIGINT, draining"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("received Ctrl-C, draining");
    }
}

/// Tear down every active session within `grace`. After the grace
/// window we return regardless — `Stopper::stop` is best-effort and
/// the daemon-side `--rm` flag prevents leaks if mica dies early.
pub async fn drain(sessions: SessionMap, grace: Duration) {
    let _ = tokio::time::timeout(grace, async {
        let ids: Vec<String> = {
            let mut v = Vec::with_capacity(sessions.len());
            sessions.each(|s| v.push(s.id().to_string()));
            v
        };
        for id in ids {
            sessions.remove(&id).await;
        }
    })
    .await;
}
