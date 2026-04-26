use arc_swap::ArcSwap;
use clap::Parser;
use mica::auth::{AuthState, AuthSwap, require_basic_auth};
use mica::backend::Backend;
use mica::backend::docker::DockerBackend;
use mica::backend::k8s::K8sBackend;
use mica::cli::Args;
use mica::config::Config;
use mica::handlers;
use mica::isolation::capability::{Capabilities, select_driver};
use mica::observability;
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

    // Phase 4: probe host capabilities and pick an isolation driver.
    let caps = Capabilities::probe();
    let driver = select_driver(Some(args.isolation.as_str()), &caps)
        .map_err(|e| anyhow::anyhow!("isolation: {e}"))?;
    tracing::info!(
        kvm = caps.kvm,
        runsc = caps.runsc,
        kata_runtime = caps.kata_runtime,
        k8s = caps.k8s_in_cluster,
        selected = driver.name(),
        "isolation capabilities probed"
    );

    let raw_backend: Arc<dyn Backend> = match args.backend.as_str() {
        "k8s" => {
            let replica = if args.replica_id.is_empty() {
                None
            } else {
                Some(args.replica_id.clone())
            };
            // --isolation translates to a K8s RuntimeClass when it's
            // a runtime-class-flavored driver. An explicit
            // --k8s-runtime-class always wins over the inferred one.
            let runtime_class = if !args.k8s_runtime_class.is_empty() {
                Some(args.k8s_runtime_class.clone())
            } else {
                driver.k8s_runtime_class().map(|s| s.to_string())
            };
            let b = K8sBackend::connect(args.k8s_namespace.clone(), replica)
                .await
                .map_err(|e| anyhow::anyhow!("k8s backend: {e}"))?
                .with_runtime_class(runtime_class.clone())
                .with_service_startup_timeout(args.service_startup_timeout);
            tracing::info!(
                namespace = %args.k8s_namespace,
                replica_id = %b.replica_id(),
                runtime_class = ?runtime_class,
                "k8s backend selected"
            );
            Arc::new(b)
        }
        _ => {
            let b = DockerBackend::connect()
                .await?
                .with_network(args.container_network.clone())
                .with_default_cpu(args.cpu.clone())
                .with_default_memory(args.memory.clone())
                .with_service_startup_timeout(args.service_startup_timeout)
                .with_save_all_logs(if args.save_all_logs {
                    Some(args.log_output_dir.clone())
                } else {
                    None
                })
                .with_disable_privileged(args.disable_privileged)
                .with_log_conf(&args.log_conf);
            Arc::new(b)
        }
    };
    let docker_backend = raw_backend;

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

    // Install Prometheus recorder (one per process). Must happen
    // before any code path records into `metrics::*` macros.
    let prom = observability::install();

    let state = AppState::new(config, args.clone(), backend).with_metrics(prom);

    if let Some(s3) = S3Uploader::from_args(&args.s3_bucket, &args.s3_region, &args.s3_prefix).await
    {
        let listener = Arc::new(UploadListener::new(Arc::new(s3)));
        state.events.add_file_listener(listener).await;
        tracing::info!(bucket = %args.s3_bucket, "S3 uploader enabled");
    }

    // Phase 5: load WASM plugins from --plugin-dir.
    if !args.plugin_dir.is_empty() {
        match mica::plugins::PluginHost::new() {
            Ok(host) => {
                let path = std::path::PathBuf::from(&args.plugin_dir);
                if let Err(e) = host.load_dir(&path).await {
                    tracing::warn!(error = %e, "plugin dir scan failed");
                } else {
                    let names = host.loaded_names().await;
                    tracing::info!(plugins = ?names, "WASM plugins loaded");
                }
            }
            Err(e) => tracing::warn!(error = %e, "wasmtime engine init failed; plugins disabled"),
        }
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

    // Basic auth gate on the WebDriver / artifact / relay surface.
    // Empty --users = no gate; any non-open path is allowed through.
    let auth: AuthSwap = match AuthState::load(&args.users) {
        Ok(s) => Arc::new(ArcSwap::from_pointee(s)),
        Err(e) => {
            anyhow::bail!("read --users {}: {e}", args.users);
        }
    };
    if !args.users.is_empty() {
        tracing::info!(path = %args.users, "HTTP Basic auth enabled");
    }
    // SIGHUP also reloads the htpasswd file alongside browsers.json.
    #[cfg(unix)]
    {
        let users_path = args.users.clone();
        let auth_for_reload = auth.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            if users_path.is_empty() {
                return;
            }
            let mut hup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(_) => return,
            };
            while hup.recv().await.is_some() {
                match AuthState::load(&users_path) {
                    Ok(s) => {
                        auth_for_reload.store(Arc::new(s));
                        tracing::info!(path = %users_path, "users file reloaded");
                    }
                    Err(e) => tracing::warn!(error = %e, "users reload failed"),
                }
            }
        });
    }

    let app = handlers::router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            auth.clone(),
            require_basic_auth,
        ))
        .layer(trace);

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
