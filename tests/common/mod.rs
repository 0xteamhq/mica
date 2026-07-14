//! Shared integration-test harness. Each test file opts in with
//! `mod common;` — helpers here replace the per-file `args()` /
//! `build_app()` copies that used to be duplicated across tests.
#![allow(dead_code)]

use arc_swap::ArcSwap;
use clap::Parser;
use mica::auth::{AuthState, AuthSwap, require_basic_auth};
use mica::backend::mock::MockBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::state::AppState;
use std::sync::Arc;

/// Default test Args, built through clap so new flags with defaults
/// never break test compilation. Override fields as needed.
pub fn args() -> Args {
    Args::parse_from(["mica", "--conf", "tests/fixtures/browsers.json"])
}

pub fn build_state(upstream: &str, args: Args) -> (AppState, Arc<MockBackend>) {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let backend = Arc::new(MockBackend::new(upstream));
    let state = AppState::new(cfg, args, backend.clone());
    (state, backend)
}

/// Full router with the auth middleware, mirroring main.rs — the
/// swap is shared with AppState so admin-API reloads/user writes
/// reach the gate, exactly like production.
pub fn build_app(state: AppState, users_path: Option<&str>) -> axum::Router {
    let auth_state = match users_path {
        Some(p) => AuthState::load(p).unwrap(),
        None => AuthState::empty(),
    };
    let auth: AuthSwap = Arc::new(ArcSwap::from_pointee(auth_state));
    let state = state.with_auth(auth.clone());
    handlers::router(state).layer(axum::middleware::from_fn_with_state(
        auth,
        require_basic_auth,
    ))
}
