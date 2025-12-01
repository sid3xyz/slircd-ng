//! slircd-ng - Straylight IRC Daemon (Next Generation)
//!
//! A high-performance, multi-threaded IRC server built on zero-copy parsing.

mod config;
mod db;
mod handlers;
mod http;
mod metrics;
mod network;
mod security;
mod services;
mod state;

use crate::config::Config;
use crate::db::Database;
use crate::network::Gateway;
use crate::services::enforce::spawn_enforcement_task;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    // Load configuration
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let config = Config::load(&config_path).map_err(|e| {
        error!(path = %config_path, error = %e, "Failed to load config");
        e
    })?;

    info!(
        server = %config.server.name,
        network = %config.server.network,
        sid = %config.server.sid,
        "Starting slircd-ng"
    );

    // Warn if using default cloak secret
    if crate::security::cloaking::is_default_secret(&config.security.cloak_secret) {
        tracing::warn!(
            "Using default cloak_secret! Set [security].cloak_secret in config.toml for production."
        );
    }

    // Initialize database
    let db_path = config
        .database
        .as_ref()
        .map(|d| d.path.as_str())
        .unwrap_or("slircd.db");
    let db = Database::new(db_path).await?;

    // Load registered channels from database
    let registered_channels: Vec<String> = db
        .channels()
        .load_all_channels()
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load registered channels from database");
            Vec::new()
        })
        .into_iter()
        .map(|r| r.name)
        .collect();
    info!(count = registered_channels.len(), "Loaded registered channels");

    // Load active shuns from database
    let active_shuns = db
        .bans()
        .get_active_shuns()
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load shuns from database");
            Vec::new()
        });
    info!(count = active_shuns.len(), "Loaded active shuns");

    // Create the Matrix (shared state)
    let matrix = Arc::new(Matrix::new(&config, registered_channels, active_shuns));

    // Initialize Prometheus metrics
    metrics::init();
    info!("Metrics initialized");

    // Start Prometheus HTTP server
    let metrics_port = config.server.metrics_port.unwrap_or(9090);
    tokio::spawn(async move {
        http::run_http_server(metrics_port).await;
    });
    info!(port = metrics_port, "Prometheus HTTP server started");

    // Start nick enforcement background task
    spawn_enforcement_task(Arc::clone(&matrix));
    info!("Nick enforcement task started");

    // Start WHOWAS cleanup task (runs every hour, removes entries older than 7 days)
    {
        let matrix = Arc::clone(&matrix);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;
                matrix.cleanup_whowas(7);
                info!("WHOWAS cleanup completed");
            }
        });
    }
    info!("WHOWAS cleanup task started");

    // Start shun expiry cleanup task (runs every minute)
    {
        let matrix = Arc::clone(&matrix);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let now = chrono::Utc::now().timestamp();
                let before = matrix.shuns.len();
                matrix.shuns.retain(|_, shun| {
                    shun.expires_at.is_none_or(|exp| exp > now)
                });
                let removed = before - matrix.shuns.len();
                if removed > 0 {
                    info!(removed = removed, "Expired shuns removed");
                }
            }
        });
    }
    info!("Shun expiry cleanup task started");

    // Start the Gateway (with optional TLS and WebSocket)
    let gateway = Gateway::bind(
        config.listen.address,
        config.tls,
        config.websocket,
        config.webirc,
        matrix,
        db,
    )
    .await?;
    gateway.run().await?;

    Ok(())
}
