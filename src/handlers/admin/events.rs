//! GET /admin/api/events — SSE stream for the dashboard.
//!
//! Two merged sources:
//!   - `AdminEvent` broadcast (session_created / session_stopped /
//!     config_reloaded / drain), and
//!   - a `stats` frame every 2s sampling the queue counters — this
//!     covers queued/pending churn without instrumenting queue.rs and
//!     doubles as a keep-alive.
//!
//! A subscriber that lags the broadcast buffer receives a `reset`
//! frame and is expected to refetch /admin/api/state.

use crate::events::AdminEvent;
use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};

const STATS_INTERVAL: Duration = Duration::from_secs(2);

pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let lifecycle = BroadcastStream::new(state.events.subscribe_admin()).map(|item| match item {
        Ok(e) => lifecycle_event(e),
        Err(BroadcastStreamRecvError::Lagged(_)) => Event::default().event("reset").data("{}"),
    });

    let stats_state = state.clone();
    // First tick fires immediately, so a fresh dashboard gets counters
    // without waiting an interval.
    let stats = IntervalStream::new(tokio::time::interval(STATS_INTERVAL))
        .map(move |_| stats_event(&stats_state));

    Sse::new(futures::stream::select(lifecycle, stats).map(Ok)).keep_alive(KeepAlive::default())
}

fn lifecycle_event(e: AdminEvent) -> Event {
    match e {
        AdminEvent::SessionCreated {
            session_id,
            browser,
            version,
            owner,
        } => Event::default().event("session_created").data(
            serde_json::json!({
                "id": session_id,
                "browser": browser,
                "version": version,
                "owner": owner,
            })
            .to_string(),
        ),
        AdminEvent::SessionStopped { session_id } => Event::default()
            .event("session_stopped")
            .data(serde_json::json!({ "id": session_id }).to_string()),
        AdminEvent::ConfigReloaded => Event::default().event("config_reloaded").data("{}"),
        AdminEvent::Drain { active } => Event::default()
            .event("drain")
            .data(serde_json::json!({ "active": active }).to_string()),
    }
}

fn stats_event(state: &AppState) -> Event {
    Event::default().event("stats").data(
        serde_json::json!({
            "total": state.queue.capacity(),
            "used": state.queue.used(),
            "queued": state.queue.queued(),
            "pending": state.queue.pending(),
            "sessions": state.sessions.len(),
            "draining": state.draining.load(Ordering::Relaxed),
        })
        .to_string(),
    )
}
