use axum::Router;
use mica::handlers::ping;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let app: Router = ping::router();
    let addr: SocketAddr = "0.0.0.0:4444".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "mica listening");
    axum::serve(listener, app).await?;
    Ok(())
}
