use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Inner WebDriver error payload (W3C §6.4 spec shape).
#[derive(Debug, Serialize)]
pub struct WdValue {
    pub error: &'static str,
    pub message: String,
    pub stacktrace: String,
}

/// W3C-shaped WebDriver error: `{"value": {"error", "message", "stacktrace"}}`.
///
/// Mirrors `selenoid/jsonerror/jsonerror.go` so existing WebDriver
/// clients see the same JSON they expect from Selenoid.
#[derive(Debug, Serialize)]
pub struct WdError {
    pub value: WdValue,
}

impl WdError {
    pub fn session_not_created(msg: impl Into<String>) -> Self {
        Self {
            value: WdValue {
                error: "session not created",
                message: msg.into(),
                stacktrace: String::new(),
            },
        }
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self {
            value: WdValue {
                error: "invalid argument",
                message: msg.into(),
                stacktrace: String::new(),
            },
        }
    }

    pub fn invalid_session_id(msg: impl Into<String>) -> Self {
        Self {
            value: WdValue {
                error: "invalid session id",
                message: msg.into(),
                stacktrace: String::new(),
            },
        }
    }

    pub fn unknown_error(msg: impl Into<String>) -> Self {
        Self {
            value: WdValue {
                error: "unknown error",
                message: msg.into(),
                stacktrace: String::new(),
            },
        }
    }
}

impl IntoResponse for WdError {
    fn into_response(self) -> Response {
        let status = match self.value.error {
            "invalid argument" => StatusCode::BAD_REQUEST,
            "invalid session id" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}
