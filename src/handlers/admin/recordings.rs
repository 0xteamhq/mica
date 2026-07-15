//! GET /admin/api/recordings — past sessions that left a recording.
//!
//! mica keeps no session history in memory: once a session ends it is
//! dropped from the SessionMap. The durable trace of a past session is
//! its artifact files — a `{id}.mp4` video (opt-in via the `enableVideo`
//! capability) and/or a `{id}.log`. This endpoint scans the video and
//! log output dirs and reconstructs the list of revisitable sessions,
//! newest first. The files themselves are served (and playable) at
//! `/video/{id}.mp4` and `/logs/{id}.log`.
//!
//! When an S3 uploader is configured artifacts may be shipped off-box
//! and removed locally; this lists what is still on local disk.

use crate::auth::RequireAdmin;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::SystemTime;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    /// Session id (the artifact filename without its extension).
    pub id: String,
    /// A `{id}.mp4` recording exists on local disk.
    pub video: bool,
    /// Size of the recording in bytes (0 when there is no video).
    pub video_bytes: u64,
    /// A `{id}.log` exists on local disk.
    pub log: bool,
    /// RFC3339 mtime of the newest artifact for this session.
    pub modified: String,
}

pub async fn list(_admin: RequireAdmin, State(state): State<AppState>) -> Json<Vec<Recording>> {
    // (video?, bytes, log?, newest-mtime) keyed by session id.
    let mut by_id: BTreeMap<String, (bool, u64, bool, Option<SystemTime>)> = BTreeMap::new();

    scan(&state.args.video_output_dir, "mp4", &mut by_id, |e, meta, len| {
        e.0 = true;
        e.1 = len;
        bump(&mut e.3, meta);
    })
    .await;
    scan(&state.args.log_output_dir, "log", &mut by_id, |e, meta, _| {
        e.2 = true;
        bump(&mut e.3, meta);
    })
    .await;

    let mut out: Vec<Recording> = by_id
        .into_iter()
        .map(|(id, (video, bytes, log, mtime))| Recording {
            id,
            video,
            video_bytes: bytes,
            log,
            modified: mtime
                .map(|t| humantime::format_rfc3339_seconds(t).to_string())
                .unwrap_or_default(),
        })
        .collect();
    // Newest first; empty mtime sorts last.
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Json(out)
}

fn bump(slot: &mut Option<SystemTime>, meta: &std::fs::Metadata) {
    if let Ok(m) = meta.modified()
        && slot.is_none_or(|cur| m > cur)
    {
        *slot = Some(m);
    }
}

/// Read `dir`, and for every `*.{ext}` file invoke `f` with the id's
/// accumulator entry, the file metadata, and its length. Missing dir is
/// treated as empty (no recordings yet).
async fn scan<F>(
    dir: &str,
    ext: &str,
    by_id: &mut BTreeMap<String, (bool, u64, bool, Option<SystemTime>)>,
    mut f: F,
) where
    F: FnMut(&mut (bool, u64, bool, Option<SystemTime>), &std::fs::Metadata, u64),
{
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name.strip_suffix(&format!(".{ext}")) else {
            continue;
        };
        let Ok(meta) = entry.metadata().await else { continue };
        if !meta.is_file() {
            continue;
        }
        let e = by_id.entry(id.to_string()).or_insert((false, 0, false, None));
        f(e, &meta, meta.len());
    }
}
