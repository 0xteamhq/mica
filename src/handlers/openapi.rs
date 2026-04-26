//! GET /openapi.yaml — embedded OpenAPI 3.0.3 description of mica's
//! HTTP surface. Generated SDKs (openapi-generator,
//! @openapitools/openapi-generator-cli, etc.) can consume this
//! directly. Source of truth lives at `deploy/openapi/mica.yaml`;
//! this file embeds it via `include_str!` so a `cargo build` is the
//! only step that ever needs to refresh the served bytes.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

const SPEC: &str = include_str!("../../deploy/openapi/mica.yaml");

pub async fn openapi_yaml() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/yaml")],
        SPEC,
    )
        .into_response()
}
