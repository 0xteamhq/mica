//! /video and /logs file endpoints.
//!
//! T39 + T40 — GET serves files from `args.video_output_dir` and
//! `args.log_output_dir`; DELETE removes them. Path traversal
//! attempts (any `..` component or absolute leading `/`) are rejected
//! with 403.

use crate::error::WdError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use std::path::{Component, PathBuf};

/// Reject a name that contains path-traversal components or is empty.
fn validate_name(name: &str) -> Result<(), WdError> {
    if name.is_empty() {
        return Err(WdError::invalid_argument("empty file name"));
    }
    let p = std::path::Path::new(name);
    for c in p.components() {
        match c {
            Component::Normal(_) => {}
            _ => {
                return Err(WdError::invalid_argument("path traversal not allowed"));
            }
        }
    }
    Ok(())
}

pub async fn get_video(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, WdError> {
    serve_file(&state.args.video_output_dir, &name).await
}

pub async fn delete_video(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, WdError> {
    delete_file(&state.args.video_output_dir, &name).await
}

pub async fn get_log(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, WdError> {
    serve_file(&state.args.log_output_dir, &name).await
}

pub async fn delete_log(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, WdError> {
    delete_file(&state.args.log_output_dir, &name).await
}

async fn serve_file(dir: &str, name: &str) -> Result<Response, WdError> {
    validate_name(name)?;
    let mut path = PathBuf::from(dir);
    path.push(name);
    let bytes = tokio::fs::read(&path).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => WdError::invalid_argument(format!("not found: {name}")),
        _ => WdError::unknown_error(format!("read {}: {e}", path.display())),
    })?;
    let ct = if name.ends_with(".mp4") {
        "video/mp4"
    } else {
        "application/octet-stream"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .body(axum::body::Body::from(bytes))
        .map_err(|e| WdError::unknown_error(format!("build response: {e}")))
}

async fn delete_file(dir: &str, name: &str) -> Result<StatusCode, WdError> {
    validate_name(name)?;
    let mut path = PathBuf::from(dir);
    path.push(name);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(WdError::unknown_error(format!(
            "delete {}: {e}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_name;

    #[test]
    fn rejects_traversal() {
        assert!(validate_name("..").is_err());
        assert!(validate_name("../etc/passwd").is_err());
        assert!(validate_name("a/../b").is_err());
        assert!(validate_name("/abs").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("ok.mp4").is_ok());
        assert!(validate_name("sid-with-dashes.log").is_ok());
    }
}
