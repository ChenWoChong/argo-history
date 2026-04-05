mod admission;
mod config;
mod model;
mod storage;
mod templates;
mod web;

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use rustls::crypto::ring::default_provider;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Settings, storage::HistoryStore, web::AppState};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, default_value = "config/config.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let settings = Settings::load(&cli.config)?;
    let store = HistoryStore::new(&settings.history.root_dir, settings.history.retention_days)?;
    let state = AppState { store };

    let http_router = web::http_router(state.clone()).layer(TraceLayer::new_for_http());
    let webhook_router = web::webhook_router(state).layer(TraceLayer::new_for_http());

    let http_addr: SocketAddr = settings
        .server
        .http_bind_addr
        .parse()
        .with_context(|| "parse http bind addr")?;
    let webhook_addr: SocketAddr = settings
        .server
        .webhook_bind_addr
        .parse()
        .with_context(|| "parse webhook bind addr")?;

    let tls = RustlsConfig::from_pem_file(
        PathBuf::from(&settings.server.tls.cert_path),
        PathBuf::from(&settings.server.tls.key_path),
    )
    .await
    .with_context(|| "load webhook tls certificate")?;

    let http_listener = tokio::net::TcpListener::bind(http_addr)
        .await
        .with_context(|| format!("bind http server {}", http_addr))?;
    let webhook_handle = Handle::new();
    let webhook_shutdown = webhook_handle.clone();

    tokio::spawn(async move {
        shutdown_signal().await;
        webhook_shutdown.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    info!(http_addr = %http_addr, webhook_addr = %webhook_addr, "starting argo-history");

    let http_server = axum::serve(http_listener, http_router.into_make_service())
        .with_graceful_shutdown(shutdown_signal());
    let webhook_server = axum_server::bind_rustls(webhook_addr, tls)
        .handle(webhook_handle)
        .serve(webhook_router.into_make_service());

    tokio::try_join!(http_server, webhook_server)?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            signal::unix::signal(signal::unix::SignalKind::terminate()).expect("register SIGTERM");
        tokio::select! {
            _ = signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        signal::ctrl_c().await.expect("register ctrl-c");
    }
}
