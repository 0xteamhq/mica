use clap::Parser;
use mica::backend::Backend;
use mica::backend::docker::DockerBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::pool::PooledBackend;
use mica::shutdown;
use mica::state::AppState;
use mica::upload::{UploadListener, s3::S3Uploader};
use std::sync::Arc;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().json().init();
    let args = Args::parse();

    let config = match Config::load(&args.conf) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %args.conf,
                "browsers.json not loaded; serving with empty config"
            );
            Config::default()
        }
    };

    let backend = DockerBackend::connect()
        .await?
        .with_network(args.container_network.clone())
        .with_default_cpu(args.cpu.clone())
        .with_default_memory(args.memory.clone())
        .with_service_startup_timeout(args.service_startup_timeout)
        .with_save_all_logs(if args.save_all_logs {
            Some(args.log_output_dir.clone())
        } else {
            None
        });
    let docker_backend: Arc<dyn Backend> = Arc::new(backend);

    // P2.3: when --warm-pool-min > 0, wrap DockerBackend with PooledBackend.
    let backend: Arc<dyn Backend> = if args.warm_pool_min > 0 {
        Arc::new(PooledBackend::new(
            docker_backend,
            args.warm_pool_min as usize,
            args.warm_pool_max as usize,
            args.warm_pool_idle_ttl,
        ))
    } else {
        docker_backend
    };

    let state = AppState::new(config, args.clone(), backend);

    if let Some(s3) = S3Uploader::from_args(&args.s3_bucket, &args.s3_region, &args.s3_prefix).await
    {
        let listener = Arc::new(UploadListener::new(Arc::new(s3)));
        state.events.add_file_listener(listener).await;
        tracing::info!(bucket = %args.s3_bucket, "S3 uploader enabled");
    }

    // T51: SIGHUP -> reload browsers.json. arc-swap lets us flip the
    // Arc<Config> without holding any locks on the hot path.
    #[cfg(unix)]
    {
        let conf_path = args.conf.clone();
        let arc_swap = state.config_swap.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let mut hup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "SIGHUP handler unavailable");
                    return;
                }
            };
            while hup.recv().await.is_some() {
                match Config::load(&conf_path) {
                    Ok(c) => {
                        arc_swap.store(Arc::new(c));
                        tracing::info!(path = %conf_path, "browsers.json reloaded");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, path = %conf_path, "reload failed; keeping previous config");
                    }
                }
            }
        });
    }

    // T49: per-request span carrying request_id (X-Request-ID or a
    // generated UUID). Surfaces in logs alongside the [SESSION_CREATED]
    // line so all events for one request stream together.
    let trace = TraceLayer::new_for_http()
        .make_span_with(
            DefaultMakeSpan::new()
                .level(tracing::Level::INFO)
                .include_headers(false),
        )
        .on_request(DefaultOnRequest::new().level(tracing::Level::DEBUG))
        .on_response(DefaultOnResponse::new().level(tracing::Level::DEBUG));

    let app = handlers::router(state.clone()).layer(trace);

    let listen = if args.listen.starts_with(':') {
        format!("0.0.0.0{}", args.listen)
    } else {
        args.listen.clone()
    };
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(addr = %listen, "mica listening");

    // T48: graceful shutdown — stop accepting new connections on
    // SIGTERM/SIGINT, then drain active sessions within
    // --graceful-period.
    let serve_state = state.clone();
    let graceful = args.graceful_period;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown::signal_future().await;
            tracing::info!(?graceful, "draining sessions");
            shutdown::drain(serve_state.sessions.clone(), graceful).await;
            tracing::info!("drain complete");
        })
        .await?;
    Ok(())
}
