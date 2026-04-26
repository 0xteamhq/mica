use clap::Parser;
use mica::backend::Backend;
use mica::backend::docker::DockerBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::state::AppState;
use mica::upload::{UploadListener, s3::S3Uploader};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().json().init();
    let args = Args::parse();

    // Config: tolerate missing file at startup; create handler will
    // surface the resulting "browser not found" if no config is loaded.
    let config = match Config::load(&args.conf) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, path = %args.conf, "browsers.json not loaded; serving with empty config");
            Config::default()
        }
    };

    // Backend: connect to Docker. CI without Docker can still build —
    // the binary fails fast at startup, which is the right behavior.
    let backend = DockerBackend::connect()
        .await?
        .with_network(args.container_network.clone())
        .with_default_cpu(args.cpu.clone())
        .with_default_memory(args.memory.clone())
        .with_service_startup_timeout(args.service_startup_timeout);
    let backend: Arc<dyn Backend> = Arc::new(backend);

    let state = AppState::new(config, args.clone(), backend);

    // M11 T47 — register the S3 uploader iff --s3-bucket is set.
    if let Some(s3) = S3Uploader::from_args(&args.s3_bucket, &args.s3_region, &args.s3_prefix).await
    {
        let listener = Arc::new(UploadListener::new(Arc::new(s3)));
        state.events.add_file_listener(listener).await;
        tracing::info!(bucket = %args.s3_bucket, "S3 uploader enabled");
    }

    let app = handlers::router(state);

    let listen = if args.listen.starts_with(':') {
        format!("0.0.0.0{}", args.listen)
    } else {
        args.listen.clone()
    };
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(addr = %listen, "mica listening");
    axum::serve(listener, app).await?;
    Ok(())
}
