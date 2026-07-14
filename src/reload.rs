//! Hot reload of operator-editable files (browsers.json + htpasswd).
//!
//! One code path serves both triggers: the SIGHUP handler in main.rs
//! and POST /admin/api/config/reload. Each file reloads independently
//! — a parse failure keeps that file's previous state and is reported
//! without blocking the other.

use crate::auth::AuthState;
use crate::config::Config;
use crate::events::AdminEvent;
use crate::quota::Quotas;
use crate::state::AppState;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct ReloadSummary {
    /// Browser names now in the registry.
    pub browsers: usize,
    /// Users in the htpasswd file (0 when auth is disabled).
    pub users: usize,
    /// Explicit per-user quota rows (0 when quotas are disabled).
    pub quotas: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    #[error("browsers.json ({path}): {message}")]
    Config { path: String, message: String },
    #[error("users file ({path}): {message}")]
    Users { path: String, message: String },
    #[error("quotas file ({path}): {message}")]
    Quotas { path: String, message: String },
}

/// Re-read browsers.json and the users file, swapping each into its
/// ArcSwap on success. Emits `AdminEvent::ConfigReloaded`.
pub fn reload_all(state: &AppState) -> Result<ReloadSummary, ReloadError> {
    let conf_path = &state.args.conf;
    let config = Config::load(conf_path).map_err(|e| ReloadError::Config {
        path: conf_path.clone(),
        message: e.to_string(),
    })?;
    let browsers = config.snapshot().len();
    state.config_swap.store(Arc::new(config));
    tracing::info!(path = %conf_path, browsers, "browsers.json reloaded");

    let users_path = &state.args.users;
    let users = if users_path.is_empty() {
        0
    } else {
        let auth = AuthState::load(users_path).map_err(|e| ReloadError::Users {
            path: users_path.clone(),
            message: e.to_string(),
        })?;
        let count = auth.user_count();
        state.auth.store(Arc::new(auth));
        tracing::info!(path = %users_path, users = count, "users file reloaded");
        count
    };

    let quotas_path = &state.args.quotas;
    let quotas = if quotas_path.is_empty() {
        // No file backing quotas: leave any API-set in-memory limits
        // alone rather than resetting them to default.
        0
    } else {
        let q = Quotas::load(quotas_path).map_err(|message| ReloadError::Quotas {
            path: quotas_path.clone(),
            message,
        })?;
        let count = q.users.len();
        state.quotas.store(q);
        tracing::info!(path = %quotas_path, rows = count, "quotas reloaded");
        count
    };

    state.events.emit_admin(AdminEvent::ConfigReloaded);
    Ok(ReloadSummary {
        browsers,
        users,
        quotas,
    })
}
