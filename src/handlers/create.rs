//! POST /wd/hub/session — create a new WebDriver session.
//!
//! Flow (T29):
//!   1. Parse caps (W3C alwaysMatch / legacy desiredCapabilities).
//!   2. Honor X-Selenoid-No-Wait + --disable-queue → try_acquire,
//!      else acquire (T14).
//!   3. Look up the requested browser+version in config.
//!   4. backend.start(StartParams) within service_startup_timeout.
//!   5. Forward POST /session to the upstream WebDriver with retry
//!      (T34): retry up to args.retry_count on transient failures.
//!   6. Extract the upstream sessionId, build the cancel hook (drops
//!      queue permit, calls backend stop, posts upstream DELETE
//!      /session/{id} best-effort), build the idle hook, register in
//!      the SessionMap.
//!   7. Promote the queue permit (pending → used).
//!   8. Log the [SESSION_CREATED] line (T35).
//!   9. Return the upstream response body unchanged.
//!
//! Cancel-on-disconnect (T33): we keep the freshly-started container
//! protected by a `StopperGuard`. If create_session returns early
//! (axum drops the future when the client disconnects, or any branch
//! returns an error), the guard's Drop spawns the Stopper so the
//! container never leaks.

use crate::backend::{StartParams, StartedSession, Stopper};
use crate::caps::Caps;
use crate::error::WdError;
use crate::events::{ArtifactKind, FileCreated, SessionStopped};
use crate::queue::Permit;
use crate::session::Session;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const NO_WAIT_HEADER: &str = "x-selenoid-no-wait";

/// Drop-guard around a Stopper. While active, dropping the guard
/// spawns the stop in the background; calling `disarm()` first
/// transfers ownership out and disables the spawn.
struct StopperGuard {
    stopper: Option<Box<dyn Stopper>>,
}

impl StopperGuard {
    fn new(stopper: Box<dyn Stopper>) -> Self {
        Self {
            stopper: Some(stopper),
        }
    }

    fn disarm(mut self) -> Box<dyn Stopper> {
        self.stopper.take().expect("disarm before drop")
    }
}

impl Drop for StopperGuard {
    fn drop(&mut self) {
        if let Some(stopper) = self.stopper.take() {
            tokio::spawn(async move { stopper.stop().await });
        }
    }
}

pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, WdError> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let started_at = Instant::now();

    // (1) Caps
    let caps = Caps::parse(&body).map_err(|e| WdError::invalid_argument(e.to_string()))?;

    // (2) Queue
    let no_wait = state.args.disable_queue
        || headers
            .get(NO_WAIT_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    let mut permit: Permit = if no_wait {
        state
            .queue
            .try_acquire()
            .ok_or_else(|| WdError::session_not_created("queue is full"))?
    } else {
        state.queue.acquire().await
    };

    // (3) Config lookup
    let browser_name = caps.browser_name.clone().unwrap_or_default();
    let browser_version = caps.browser_version.clone();
    let (browser, version) = state
        .config
        .find(&browser_name, browser_version.as_deref())
        .ok_or_else(|| {
            WdError::session_not_created(format!(
                "browser not found: {browser_name} {}",
                browser_version.as_deref().unwrap_or("")
            ))
        })?;

    // (4) Backend start
    let started: StartedSession = state
        .backend
        .start(StartParams {
            request_id: request_id.clone(),
            caps: caps.clone(),
            browser: browser.clone(),
            version: version.clone(),
        })
        .await
        .map_err(WdError::from)?;
    let upstream = started.upstream.clone();
    let container_id = started.container_id.clone();
    let host_ports = started.host_ports.clone();
    // Arm the cancel-on-disconnect guard immediately.
    let guard = StopperGuard::new(started.stopper);

    // (5) Forward POST /session with retry.
    let upstream_resp =
        forward_create(&state.http, &upstream, &body, state.args.retry_count).await?;

    // (6) Extract session id (W3C: value.sessionId; legacy: top-level
    // sessionId).
    let session_id = upstream_resp
        .get("value")
        .and_then(|v| v.get("sessionId"))
        .or_else(|| upstream_resp.get("sessionId"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| WdError::session_not_created("upstream response missing sessionId"))?
        .to_string();

    // Disarm the guard — ownership of the stopper moves into the
    // session's cancel hook. From here on the session map owns
    // teardown.
    let stopper = guard.disarm();

    // Build the cancel hook fired on session removal. Promote first
    // so the permit moves from pending -> used; then own the permit
    // inside the closure so dropping the closure releases the slot.
    permit.promote();
    let permit_holder: Arc<std::sync::Mutex<Option<Permit>>> =
        Arc::new(std::sync::Mutex::new(Some(permit)));
    let permit_for_cancel = permit_holder.clone();
    let http = state.http.clone();
    let upstream_for_cancel = upstream.clone();
    let session_id_for_cancel = session_id.clone();
    let events_for_cancel = state.events.clone();
    let browser_name_for_cancel = browser_name.clone();
    let version_for_cancel = version.clone();
    let session_started = SystemTime::now();
    let video_dir = state.args.video_output_dir.clone();
    let log_dir = state.args.log_output_dir.clone();
    let cancel = Box::new(move || {
        // Release the queue slot regardless of stopper outcome.
        if let Ok(mut g) = permit_for_cancel.lock() {
            g.take();
        }
        // Best-effort upstream DELETE + backend stop. T43: emit
        // SessionStopped after stop, T44: emit FileCreated for any
        // finalized artifact. All in the background since cancel
        // must be sync.
        let http = http.clone();
        let upstream = upstream_for_cancel.clone();
        let sid = session_id_for_cancel.clone();
        let events = events_for_cancel.clone();
        let browser = browser_name_for_cancel.clone();
        let version = version_for_cancel.clone();
        let video_dir = video_dir.clone();
        let log_dir = log_dir.clone();
        tokio::spawn(async move {
            let _ = http
                .delete(format!("{upstream}/session/{sid}"))
                .send()
                .await;
            stopper.stop().await;
            // T44 — emit FileCreated for whichever artifacts exist.
            for (kind, dir, ext) in [
                (ArtifactKind::Video, video_dir.as_str(), "mp4"),
                (ArtifactKind::Log, log_dir.as_str(), "log"),
            ] {
                let path = PathBuf::from(dir).join(format!("{sid}.{ext}"));
                if tokio::fs::metadata(&path).await.is_ok() {
                    events
                        .emit_file(FileCreated {
                            path,
                            session_id: sid.clone(),
                            kind,
                        })
                        .await;
                }
            }
            // T43 — SessionStopped goes out last so listeners that
            // upload artifacts can rely on FileCreated having fired.
            events
                .emit_session(SessionStopped {
                    session_id: sid,
                    started: session_started,
                    finished: SystemTime::now(),
                    browser: Some(browser),
                    browser_version: Some(version),
                })
                .await;
        });
    });

    // Build the idle hook that triggers SessionMap::remove (which in
    // turn fires the cancel hook above).
    let sessions = state.sessions.clone();
    let session_id_for_idle = session_id.clone();
    let on_idle = Box::new(move || {
        let sessions = sessions.clone();
        let sid = session_id_for_idle.clone();
        tokio::spawn(async move {
            sessions.remove(&sid).await;
        });
    });

    // (7) Register the session.
    let session = Session::new_full(
        &session_id,
        upstream.clone(),
        host_ports,
        browser_name.clone(),
        version.clone(),
        state.args.timeout,
        on_idle,
        cancel,
    );
    state.sessions.put(session).await;

    // (8) [SESSION_CREATED] log line — Selenoid-compatible structured form.
    let elapsed_ms = started_at.elapsed().as_millis();
    tracing::info!(
        request_id = %request_id,
        elapsed_ms = elapsed_ms,
        browser = %browser_name,
        version = %version,
        container = %container_id,
        session_id = %session_id,
        "[SESSION_CREATED]"
    );

    // (9) Return the upstream body unchanged.
    Ok(Json(upstream_resp))
}

async fn forward_create(
    http: &reqwest::Client,
    upstream: &str,
    body: &Value,
    retry_count: u32,
) -> Result<Value, WdError> {
    let url = format!("{upstream}/session");
    let mut last_err: Option<String> = None;
    for attempt in 0..=retry_count {
        match http.post(&url).json(body).send().await {
            Ok(resp) if resp.status().is_success() => {
                return resp
                    .json::<Value>()
                    .await
                    .map_err(|e| WdError::session_not_created(format!("parse upstream: {e}")));
            }
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                last_err = Some(format!("upstream {status}: {text}"));
                if !status.is_server_error() {
                    // 4xx: don't retry.
                    break;
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        if attempt < retry_count {
            tokio::time::sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
        }
    }
    Err(WdError::session_not_created(
        last_err.unwrap_or_else(|| "upstream error".to_string()),
    ))
}
