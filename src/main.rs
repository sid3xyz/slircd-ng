//! slircd-ng - Straylight IRC Daemon (Next Generation)
//!
//! A high-performance, multi-threaded IRC server built on zero-copy parsing.

mod config;
mod db;
mod handlers;
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

    // Create the Matrix (shared state)
    let matrix = Arc::new(Matrix::new(&config));

    // Start nick enforcement background task
    spawn_enforcement_task(Arc::clone(&matrix));
    info!("Nick enforcement task started");

    // Start the Gateway (with optional TLS and WebSocket)
    let gateway = Gateway::bind(
        config.listen.address,
        config.tls,
        config.websocket,
        matrix,
        db,
    )
    .await?;
    gateway.run().await?;

    Ok(())
}
