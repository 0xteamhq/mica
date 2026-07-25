//! POST /wd/hub/session — create a new WebDriver session.
//!
//! Flow (T29):
//!   1. Parse caps (W3C alwaysMatch / legacy desiredCapabilities).
//!   2. Honor X-Mica-No-Wait + --disable-queue → try_acquire,
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

use crate::auth::AuthedUser;
use crate::backend::{StartParams, StartedSession, Stopper};
use crate::caps::Caps;
use crate::error::WdError;
use crate::events::{AdminEvent, ArtifactKind, FileCreated, SessionStopped};
use crate::observability::names::{
    SESSION_CREATE_DURATION_MS, SESSIONS_CREATED_TOTAL, SESSIONS_FAILED_TOTAL,
    SESSIONS_TEARDOWN_TOTAL,
};
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

const NO_WAIT_HEADER: &str = "x-mica-no-wait";

/// What the session's cancel hook owns and releases on teardown:
/// the queue slot and (for authenticated sessions) the user's quota
/// unit. One take() drops both.
type SlotHolder = Arc<std::sync::Mutex<Option<(Permit, Option<crate::quota::QuotaGuard>)>>>;

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
    state_ext: State<AppState>,
    owner: Option<axum::Extension<AuthedUser>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, WdError> {
    let started_at = Instant::now();
    let owner = owner.map(|axum::Extension(u)| u.name);
    let result = create_session_inner(state_ext, owner, headers, body).await;
    let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    metrics::histogram!(SESSION_CREATE_DURATION_MS).record(elapsed_ms);
    match &result {
        Ok(_) => metrics::counter!(SESSIONS_CREATED_TOTAL).increment(1),
        Err(e) => metrics::counter!(SESSIONS_FAILED_TOTAL, "error" => e.value.error).increment(1),
    }
    result
}

async fn create_session_inner(
    State(state): State<AppState>,
    owner: Option<String>,
    headers: HeaderMap,
    body: Value,
) -> Result<Json<Value>, WdError> {
    // Draining (manual /admin/api/drain or graceful shutdown):
    // existing sessions keep proxying, new ones are rejected.
    if state.draining.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(WdError::session_not_created("node is draining"));
    }
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let started_at = Instant::now();

    // (1) Caps
    let mut caps = Caps::parse(&body).map_err(|e| WdError::invalid_argument(e.to_string()))?;

    // (1b) session.on-create plugin chain. Runs BEFORE queue acquire
    // so a reject doesn't burn a permit. The chain runs under the
    // `--plugin-on-create-timeout` budget; exceeding it rejects.
    if let Some(host) = state.plugins.as_ref() {
        let preliminary_session_id = uuid::Uuid::new_v4().to_string();
        match host
            .session_decision(
                &preliminary_session_id,
                &caps,
                state.args.plugin_on_create_timeout,
            )
            .await
        {
            crate::plugins::SessionDecision::Accept(new_caps) => {
                caps = *new_caps;
            }
            crate::plugins::SessionDecision::Reject(reason) => {
                return Err(WdError::session_not_created(reason));
            }
        }
    }

    // (1c) Per-user quota — checked BEFORE the queue so an over-quota
    // request fails fast without burning a slot. Sessions without an
    // owner (auth disabled) bypass quotas. The guard's Drop releases
    // the unit; it rides in the permit holder below so its lifetime
    // exactly matches the session's queue slot.
    let quota_guard = match owner.as_deref() {
        Some(user) => match state.quotas.try_acquire(user) {
            Some(g) => Some(g),
            None => {
                let limit = state.quotas.snapshot().limit_for(user);
                return Err(WdError::session_not_created(format!(
                    "user quota exceeded ({limit} concurrent sessions)"
                )));
            }
        },
        None => None,
    };

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
        .config()
        .find(&browser_name, browser_version.as_deref())
        .ok_or_else(|| {
            WdError::session_not_created(format!(
                "browser not found: {browser_name} {}",
                browser_version.as_deref().unwrap_or("")
            ))
        })?;

    // Cap the per-session idle timeout that the client can request
    // via `mica:options.sessionTimeout`. If unset, fall back to
    // --timeout. If the requested value exceeds --max-timeout,
    // clamp it.
    let effective_timeout = match caps.session_timeout.as_deref() {
        Some(s) if !s.is_empty() => match humantime::parse_duration(s) {
            Ok(d) if d <= state.args.max_timeout => d,
            Ok(_) => state.args.max_timeout,
            Err(_) => state.args.timeout,
        },
        _ => state.args.timeout,
    };

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

    // Platform reported by the upstream driver (chromedriver etc.) —
    // "linux" / "mac" / "windows". Recorded in the artifact metadata
    // sidecar so the dashboard can filter recordings by OS.
    let platform = upstream_resp
        .pointer("/value/capabilities/platformName")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // Disarm the guard — ownership of the stopper moves into the
    // session's cancel hook. From here on the session map owns
    // teardown.
    let stopper = guard.disarm();

    // Build the cancel hook fired on session removal. Promote first
    // so the permit moves from pending -> used; then own the permit
    // inside the closure so dropping the closure releases the slot.
    permit.promote();
    // The quota guard shares the permit's lifetime: the cancel hook's
    // take() drops both, releasing the queue slot and the user's
    // quota unit together.
    let permit_holder: SlotHolder = Arc::new(std::sync::Mutex::new(Some((permit, quota_guard))));
    let permit_for_cancel = permit_holder.clone();
    let http = state.http.clone();
    let upstream_for_cancel = upstream.clone();
    let session_id_for_cancel = session_id.clone();
    let request_id_for_cancel = request_id.clone();
    let events_for_cancel = state.events.clone();
    let browser_name_for_cancel = browser_name.clone();
    let version_for_cancel = version.clone();
    let owner_for_cancel = owner.clone();
    let platform_for_cancel = platform.clone();
    let session_started = SystemTime::now();
    let started_rfc3339 = humantime::format_rfc3339_seconds(session_started).to_string();
    let video_dir = state.args.video_output_dir.clone();
    let log_dir = state.args.log_output_dir.clone();
    let delete_timeout = state.args.session_delete_timeout;
    let s3_key_pattern_for_cancel = caps.s3_key_pattern.clone();
    let plugins_for_cancel = state.plugins.clone();
    let cancel = Box::new(move || {
        metrics::counter!(SESSIONS_TEARDOWN_TOTAL).increment(1);
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
        let request_id = request_id_for_cancel.clone();
        let events = events_for_cancel.clone();
        let browser = browser_name_for_cancel.clone();
        let version = version_for_cancel.clone();
        let owner = owner_for_cancel.clone();
        let platform = platform_for_cancel.clone();
        let started_rfc3339 = started_rfc3339.clone();
        let video_dir = video_dir.clone();
        let log_dir = log_dir.clone();
        tokio::spawn(async move {
            let _ = http
                .delete(format!("{upstream}/session/{sid}"))
                .timeout(delete_timeout)
                .send()
                .await;
            stopper.stop().await;
            // A self-recording browser image names its .mp4 by the
            // request id (the upstream session id isn't known at
            // container start). Now that the container has stopped and
            // ffmpeg has finalized the file, rename it to
            // `{session_id}.mp4` so pickup, /video, and the UI all stay
            // keyed on the session id.
            let recorded = PathBuf::from(&video_dir).join(format!("{request_id}.mp4"));
            if tokio::fs::metadata(&recorded).await.is_ok() {
                let target = PathBuf::from(&video_dir).join(format!("{sid}.mp4"));
                if let Err(e) = tokio::fs::rename(&recorded, &target).await {
                    tracing::warn!(error = %e, %request_id, %sid, "rename recording failed");
                }
            }
            // Metadata sidecar: mica keeps no session history, so record
            // the browser/version/platform/owner/started alongside the
            // artifact for the dashboard's Recordings filters. Written
            // only when an artifact exists, so it never litters the dir.
            let has_video =
                tokio::fs::metadata(PathBuf::from(&video_dir).join(format!("{sid}.mp4")))
                    .await
                    .is_ok();
            let has_log = tokio::fs::metadata(PathBuf::from(&log_dir).join(format!("{sid}.log")))
                .await
                .is_ok();
            if has_video || has_log {
                let meta = serde_json::json!({
                    "browser": browser,
                    "version": version,
                    "platform": platform,
                    "owner": owner,
                    "started": started_rfc3339,
                });
                let meta_path = PathBuf::from(&video_dir).join(format!("{sid}.json"));
                if let Ok(bytes) = serde_json::to_vec_pretty(&meta)
                    && let Err(e) = tokio::fs::write(&meta_path, bytes).await
                {
                    tracing::warn!(error = %e, %sid, "write recording metadata failed");
                }
            }
            // T44 — for each finalized artifact: ask the plugin
            // chain first (if any), then either emit to the EventBus
            // (Keep → S3Uploader runs) or short-circuit per the
            // plugin verdict.
            for (kind, dir, ext) in [
                (ArtifactKind::Video, video_dir.as_str(), "mp4"),
                (ArtifactKind::Log, log_dir.as_str(), "log"),
            ] {
                let path = PathBuf::from(dir).join(format!("{sid}.{ext}"));
                if tokio::fs::metadata(&path).await.is_ok() {
                    let event = FileCreated {
                        path: path.clone(),
                        session_id: sid.clone(),
                        kind,
                        browser: Some(browser.clone()),
                        browser_version: Some(version.clone()),
                        s3_key_pattern: s3_key_pattern_for_cancel.clone(),
                    };
                    let verdict = match &plugins_for_cancel {
                        Some(host) => host.artifact_verdict(&event).await,
                        None => crate::plugins::ArtifactVerdict::Keep,
                    };
                    match verdict {
                        crate::plugins::ArtifactVerdict::Keep => {
                            events.emit_file(event).await;
                        }
                        crate::plugins::ArtifactVerdict::Skip => {
                            if let Err(e) = tokio::fs::remove_file(&path).await {
                                tracing::warn!(error = %e, path = %path.display(), "plugin requested skip; remove failed");
                            }
                        }
                        crate::plugins::ArtifactVerdict::S3 { .. }
                        | crate::plugins::ArtifactVerdict::CustomUri(_) => {
                            // Plugin handled (or directed) the
                            // upload itself. Skip the default
                            // S3Uploader for this artifact.
                        }
                    }
                }
            }
            // T43 — SessionStopped goes out last so listeners that
            // upload artifacts can rely on FileCreated having fired.
            let finished = SystemTime::now();
            events
                .emit_session(SessionStopped {
                    session_id: sid.clone(),
                    started: session_started,
                    finished,
                    browser: Some(browser.clone()),
                    browser_version: Some(version.clone()),
                })
                .await;
            events.emit_admin(AdminEvent::SessionStopped {
                session_id: sid.clone(),
            });
            // session.on-end notification — best-effort fan-out to
            // every plugin. Already inside a tokio::spawn so a slow
            // plugin can't stall the cancel hook.
            if let Some(host) = plugins_for_cancel.as_ref() {
                host.dispatch_session_end(
                    &sid,
                    session_started,
                    finished,
                    Some(browser),
                    Some(version),
                )
                .await;
            }
        });
    });

    // Build the idle hook that triggers SessionMap::remove (which in
    // turn fires the cancel hook above). Emits a structured
    // [SESSION_TIMED_OUT] log line when the reaper fires.
    let sessions = state.sessions.clone();
    let session_id_for_idle = session_id.clone();
    let request_id_for_idle = request_id.clone();
    let on_idle = Box::new(move || {
        tracing::info!(
            request_id = %request_id_for_idle,
            session_id = %session_id_for_idle,
            "[SESSION_TIMED_OUT]"
        );
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
        owner.clone(),
        effective_timeout,
        on_idle,
        cancel,
    );
    state.sessions.put(session).await;

    // (8) [SESSION_CREATED] log line + admin dashboard event.
    state.events.emit_admin(AdminEvent::SessionCreated {
        session_id: session_id.clone(),
        browser: browser_name.clone(),
        version: version.clone(),
        owner,
    });
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
