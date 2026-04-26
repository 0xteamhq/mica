use clap::Parser;
use mica::cli::Args;
use std::time::Duration;

#[test]
fn parses_defaults() {
    let args = Args::try_parse_from(["mica"]).expect("parse");
    assert_eq!(args.listen, ":4444");
    assert_eq!(args.conf, "config/browsers.json");
    assert_eq!(args.limit, 5);
    assert_eq!(args.timeout, Duration::from_secs(60));
    assert_eq!(args.service_startup_timeout, Duration::from_secs(30));
    assert_eq!(args.session_attempt_timeout, Duration::from_secs(30));
    assert_eq!(args.retry_count, 1);
    assert_eq!(args.video_output_dir, "video");
    assert_eq!(args.log_output_dir, "logs");
    assert_eq!(args.container_network, "default");
    assert_eq!(args.cpu, "");
    assert_eq!(args.memory, "");
    assert!(!args.enable_file_upload);
    assert!(!args.disable_queue);
    assert_eq!(args.graceful_period, Duration::from_secs(300));
    assert!(!args.save_all_logs);
}

#[test]
fn parses_custom_limit_and_timeout() {
    let args = Args::try_parse_from(["mica", "--limit", "20", "--timeout", "90s"]).expect("parse");
    assert_eq!(args.limit, 20);
    assert_eq!(args.timeout, Duration::from_secs(90));
}

#[test]
fn rejects_negative_limit() {
    let result = Args::try_parse_from(["mica", "--limit", "-1"]);
    assert!(result.is_err());
}

#[test]
fn parses_bool_flags() {
    let args = Args::try_parse_from([
        "mica",
        "--enable-file-upload",
        "--disable-queue",
        "--save-all-logs",
    ])
    .expect("parse");
    assert!(args.enable_file_upload);
    assert!(args.disable_queue);
    assert!(args.save_all_logs);
}

#[test]
fn parses_graceful_period() {
    let args = Args::try_parse_from(["mica", "--graceful-period", "120s"]).expect("parse");
    assert_eq!(args.graceful_period, Duration::from_secs(120));
}
