use clap::Parser;
use std::time::Duration;

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

#[derive(Parser, Debug, Clone)]
#[command(name = "mica", version, about = "Browser grid for the T-system")]
pub struct Args {
    /// Listen address.
    #[arg(long, default_value = ":4444", env = "MICA_LISTEN")]
    pub listen: String,

    /// Path to browsers.json.
    #[arg(long, default_value = "config/browsers.json", env = "MICA_CONF")]
    pub conf: String,

    /// Max parallel sessions.
    #[arg(long, default_value_t = 5)]
    pub limit: u32,

    /// Session idle timeout.
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    pub timeout: Duration,

    /// Service startup timeout.
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    pub service_startup_timeout: Duration,

    /// New-session attempt timeout.
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    pub session_attempt_timeout: Duration,

    /// Number of new-session retries.
    #[arg(long, default_value_t = 1)]
    pub retry_count: u32,

    /// Video output directory.
    #[arg(long, default_value = "video")]
    pub video_output_dir: String,

    /// Log output directory.
    #[arg(long, default_value = "logs")]
    pub log_output_dir: String,

    /// Container network.
    #[arg(long, default_value = "default")]
    pub container_network: String,

    /// Default CPU limit (empty = unlimited).
    #[arg(long, default_value = "")]
    pub cpu: String,

    /// Default memory limit (empty = unlimited).
    #[arg(long, default_value = "")]
    pub memory: String,

    /// Enable /file upload endpoint.
    #[arg(long, default_value_t = false)]
    pub enable_file_upload: bool,

    /// Disable the request queue (return 429 instead of blocking).
    #[arg(long, default_value_t = false)]
    pub disable_queue: bool,

    /// Graceful shutdown period.
    #[arg(long, default_value = "300s", value_parser = parse_duration)]
    pub graceful_period: Duration,

    /// Save all container logs (not only sessions with `enableLog`).
    #[arg(long, default_value_t = false)]
    pub save_all_logs: bool,
}
