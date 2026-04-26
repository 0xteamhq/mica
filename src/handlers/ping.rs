//! GET /ping — liveness + minimal grid stats.
//!
//! Two router exports:
//! - `router()` — standalone (no state), used by `tests/ping_test.rs`.
//!   Returns the Pong shape with zeros for state-aware fields.
//! - `with_state` — for the live binary; surfaces real session and
//!   queue counters (T37).

use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

static START: OnceLock<SystemTime> = OnceLock::new();

fn start_time() -> SystemTime {
    *START.get_or_init(SystemTime::now)
}

#[derive(Serialize)]
pub struct Pong {
    pub uptime: String,
    #[serde(rename = "lastReloadTime")]
    pub last_reload_time: String,
    pub version: &'static str,
    pub sessions: usize,
    #[serde(rename = "queue")]
    pub queue: QueueCounters,
}

#[derive(Serialize, Default)]
pub struct QueueCounters {
    pub total: usize,
    pub used: usize,
    pub queued: usize,
    pub pending: usize,
}

fn build(sessions: usize, queue: QueueCounters) -> Pong {
    let elapsed = SystemTime::now()
        .duration_since(start_time())
        .unwrap_or(Duration::ZERO);
    Pong {
        uptime: humantime::format_duration(elapsed).to_string(),
        last_reload_time: humantime::format_rfc3339(start_time()).to_string(),
        version: env!("CARGO_PKG_VERSION"),
        sessions,
        queue,
    }
}

pub async fn ping() -> Json<Pong> {
    Json(build(0, QueueCounters::default()))
}

pub async fn ping_with_state(State(state): State<AppState>) -> Json<Pong> {
    Json(build(
        state.sessions.len(),
        QueueCounters {
            total: state.queue.capacity(),
            used: state.queue.used(),
            queued: state.queue.queued(),
            pending: state.queue.pending(),
        },
    ))
}

pub fn router() -> Router {
    Router::new().route("/ping", get(ping))
}
