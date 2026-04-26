use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

static START: OnceLock<SystemTime> = OnceLock::new();

fn start_time() -> SystemTime {
    *START.get_or_init(SystemTime::now)
}

#[derive(Serialize)]
struct Pong {
    uptime: String,
    #[serde(rename = "lastReloadTime")]
    last_reload_time: String,
    version: &'static str,
}

async fn ping() -> Json<Pong> {
    let elapsed = SystemTime::now()
        .duration_since(start_time())
        .unwrap_or(Duration::ZERO);
    Json(Pong {
        uptime: humantime::format_duration(elapsed).to_string(),
        last_reload_time: humantime::format_rfc3339(start_time()).to_string(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub fn router() -> Router {
    Router::new().route("/ping", get(ping))
}
