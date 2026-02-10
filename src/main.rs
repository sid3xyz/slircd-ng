//! slircd-ng - Straylight IRC Daemon (Next Generation)
//!
//! A high-performance, multi-threaded IRC server built on zero-copy parsing.

mod caps;
mod config;
mod db;
mod error;
mod handlers;
mod history;
mod http;
mod metrics;
mod network;
mod security;
mod services;
mod state;
mod sync;
mod telemetry;

use crate::config::Config;
use crate::db::Database;
use crate::handlers::Registry;
use crate::network::Gateway;
use crate::state::Matrix;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

/// Resolve the configuration path from CLI arguments.
/// Supports `-c <path>`, `--config <path>`, or a bare path.
/// Falls back to `config.toml` when no argument is provided.
fn resolve_config_path() -> String {
    let mut args = std::env::args().skip(1);

    let raw_path = match args.next() {
        Some(flag) if flag == "-c" || flag == "--config" => args.next().unwrap_or_else(|| {
            eprintln!("Missing path after {}", flag);
            std::process::exit(1);
        }),
        Some(path) => path,
        None => "config.toml".to_string(),
    };

    // Canonicalize to avoid relying on the current working directory during REHASH.
    match std::fs::canonicalize(Path::new(&raw_path)) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => raw_path,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration first (before tracing, so we can use log_format)
    let config_path = resolve_config_path();

    let config = Config::load(&config_path).map_err(|e| {
        eprintln!("ERROR: Failed to load config from {}: {}", config_path, e);
        e
    })?;

    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize tracing based on config
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match config.server.log_format {
        crate::config::LogFormat::Json => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .json()
                .init();
        }
        crate::config::LogFormat::Pretty => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .init();
        }
    }

    // Validate configuration
    if let Err(errors) = crate::config::validate(&config) {
        for err in &errors {
            error!(error = %err, "Configuration validation failed");
        }
        return Err(anyhow::anyhow!(
            "Configuration validation failed with {} error(s)",
            errors.len()
        ));
    }

    info!(
        server = %config.server.name,
        network = %config.server.network,
        sid = %config.server.sid,
        "Starting slircd-ng"
    );

    // SECURITY: Refuse to start with default/weak cloak secret
    // This prevents operators from accidentally running in production with predictable IP cloaks
    if crate::security::cloaking::is_default_secret(&config.security.cloak_secret) {
        // Check for explicit override via environment variable (for testing/dev only)
        if std::env::var("SLIRCD_ALLOW_INSECURE_CLOAK").is_ok() {
            tracing::warn!(
                "⚠️  INSECURE: Running with weak cloak_secret (allowed via SLIRCD_ALLOW_INSECURE_CLOAK)"
            );
        } else {
            error!("FATAL: Insecure cloak_secret detected!");
            error!("  The cloak_secret is used to hash user IP addresses for privacy.");
            error!("  Using a weak or default secret makes IP cloaks predictable and reversible.");
            error!("");
            error!("  To fix, set a strong secret in config.toml:");
            error!("    [security]");
            error!("    cloak_secret = \"<random-32-char-string>\"");
            error!("");
            error!("  Generate a secure secret with:");
            error!("    openssl rand -hex 32");
            error!("");
            error!("  For testing only, set SLIRCD_ALLOW_INSECURE_CLOAK=1 to bypass this check.");
            return Err(anyhow::anyhow!(
                "Refusing to start with insecure cloak_secret. See error messages above."
            ));
        }
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
    info!(
        count = registered_channels.len(),
        "Loaded registered channels"
    );

    // Load active shuns from database
    let active_shuns = db.bans().get_active_shuns().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load shuns from database");
        Vec::new()
    });
    info!(count = active_shuns.len(), "Loaded active shuns");

    // Load active bans from database for connection-time checks
    let active_klines = db.bans().get_active_klines().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load K-lines from database");
        Vec::new()
    });
    let active_dlines = db.bans().get_active_dlines().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load D-lines from database");
        Vec::new()
    });
    let active_glines = db.bans().get_active_glines().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load G-lines from database");
        Vec::new()
    });
    let active_zlines = db.bans().get_active_zlines().await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load Z-lines from database");
        Vec::new()
    });
    info!(
        klines = active_klines.len(),
        dlines = active_dlines.len(),
        glines = active_glines.len(),
        zlines = active_zlines.len(),
        "Loaded active bans into cache"
    );

    // Initialize history provider and always-on store
    let (history, always_on_store): (
        Arc<dyn crate::history::HistoryProvider>,
        Option<Arc<crate::db::AlwaysOnStore>>,
    ) = if config.history.enabled {
        match config.history.backend.as_str() {
            "redb" => {
                info!(path = %config.history.path, "Initializing Redb history backend");
                let redb_provider = crate::history::redb::RedbProvider::new(&config.history.path)?;
                let redb_db = redb_provider.database();

                // Create AlwaysOnStore sharing the same Redb database
                let store = match crate::db::AlwaysOnStore::new(redb_db) {
                    Ok(store) => {
                        info!("AlwaysOn store initialized (sharing Redb database)");
                        Some(Arc::new(store))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to initialize AlwaysOn store, continuing without persistence");
                        None
                    }
                };

                (Arc::new(redb_provider), store)
            }
            _ => {
                info!("History backend 'none' or unknown. Using NoOp.");
                (Arc::new(crate::history::noop::NoOpProvider), None)
            }
        }
    } else {
        info!("History disabled. Using NoOp provider.");
        (Arc::new(crate::history::noop::NoOpProvider), None)
    };

    // Create the Matrix (shared state)
    // Use database directory for data files (IP deny list, etc.)
    let data_dir = std::path::Path::new(db_path).parent();

    // Disconnect worker: channel actors can request disconnects without blocking.
    // Use bounded channel with backpressure to prevent memory exhaustion from
    // disconnect storms. 1024 slots should handle burst disconnects while
    // preventing unbounded memory growth.
    const DISCONNECT_CHANNEL_SIZE: usize = 1024;
    let (disconnect_tx, mut disconnect_rx) =
        tokio::sync::mpsc::channel::<(String, String)>(DISCONNECT_CHANNEL_SIZE);
    let (matrix_struct, mut router_rx) = Matrix::new(crate::state::MatrixParams {
        config: &config,
        config_path: config_path.clone(),
        data_dir,
        db: db.clone(),
        history,
        registered_channels,
        shuns: active_shuns,
        klines: active_klines,
        dlines: active_dlines,
        glines: active_glines,
        zlines: active_zlines,
        disconnect_tx,
        always_on_store: always_on_store.clone(),
    });
    let matrix = Arc::new(matrix_struct);
    info!("Matrix initialized");

    // Spawn all background tasks
    matrix
        .lifecycle_manager
        .spawn_background_tasks(Arc::clone(&matrix));
    info!("Background tasks started");

    // Spawn router task for remote messages
    {
        let matrix = Arc::clone(&matrix);
        tokio::spawn(async move {
            while let Some(msg_arc) = router_rx.recv().await {
                let mut msg = (*msg_arc).clone();
                // Check for x-target-uid tag
                let target_uid = msg
                    .tags
                    .as_ref()
                    .and_then(|tags| tags.iter().find(|t| t.0 == "x-target-uid"))
                    .and_then(|t| t.1.as_ref())
                    .cloned();

                let target_uid = if let Some(uid) = target_uid {
                    // Rewrite command to target UID
                    match &msg.command {
                        slirc_proto::Command::PRIVMSG(_, text) => {
                            msg.command = slirc_proto::Command::PRIVMSG(uid.clone(), text.clone());
                        }
                        slirc_proto::Command::NOTICE(_, text) => {
                            msg.command = slirc_proto::Command::NOTICE(uid.clone(), text.clone());
                        }
                        _ => {}
                    }
                    uid
                } else {
                    // Fallback to command target (if it's a UID)
                    match &msg.command {
                        slirc_proto::Command::PRIVMSG(target, _) => target.clone(),
                        slirc_proto::Command::NOTICE(target, _) => target.clone(),
                        _ => continue,
                    }
                };

                info!(target_uid = %target_uid, "Router task received message");

                // Look up server for target UID
                // Assuming UID prefix is SID (3 chars)
                if target_uid.len() >= 3 {
                    let sid_prefix = &target_uid[0..3];
                    let target_sid =
                        slirc_proto::sync::clock::ServerId::new(sid_prefix.to_string());

                    if let Some(peer) = matrix.sync_manager.get_peer_for_server(&target_sid) {
                        info!(target_sid = %target_sid.as_str(), "Routing message to peer");
                        if let Err(e) = peer.tx.send(Arc::new(msg)).await {
                            tracing::warn!(target_sid = %target_sid.as_str(), error = %e, "Failed to route message to peer (link likely dead)");
                        }
                    } else {
                        tracing::warn!(target_sid = %target_sid.as_str(), "No peer found for target server");
                    }
                }
            }
        });
    }

    // Process disconnect requests outside of channel actor tasks to avoid deadlocks.
    {
        let matrix = Arc::clone(&matrix);
        tokio::spawn(async move {
            while let Some((uid, reason)) = disconnect_rx.recv().await {
                // Disconnect is asynchronous, but we should log if it somehow fails (unlikely since it returns Vec<String>)
                matrix.disconnect_user(&uid, &reason).await;
            }
        });
    }

    // Prometheus metrics are optional.
    // Convention: metrics_port = 0 disables the HTTP endpoint (used by tests).
    let metrics_port = config.server.metrics_port.unwrap_or(9090);
    if metrics_port == 0 {
        info!("Metrics disabled");
    } else {
        metrics::init();
        info!("Metrics initialized");

        tokio::spawn(async move {
            http::run_http_server(metrics_port).await;
        });
        info!(port = metrics_port, "Prometheus HTTP server started");
    }

    // Restore always-on clients from persistent storage
    {
        let restored = matrix.client_manager.restore_from_storage().await;
        match restored {
            Ok(count) if count > 0 => {
                info!(count = count, "Restored always-on clients from storage");
            }
            Ok(_) => {
                info!("No always-on clients to restore");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to restore always-on clients");
            }
        }
    }

    // Create command handler registry
    let registry = Arc::new(Registry::new(config.webirc.clone()));

    // Start the Gateway (with optional TLS and WebSocket)
    let gateway = Gateway::bind(
        config.listen,
        config.tls,
        config.websocket,
        matrix.clone(),
        registry.clone(),
        db.clone(),
    )
    .await?;

    // Start outgoing connections
    for link in &config.links {
        if link.autoconnect {
            matrix.sync_manager.connect_to_peer(
                matrix.clone(),
                registry.clone(),
                db.clone(),
                link.clone(),
            );
        }
    }

    // Start inbound S2S listener (TLS and/or plaintext)
    matrix.sync_manager.start_inbound_listener(
        matrix.clone(),
        registry.clone(),
        db.clone(),
        config.s2s_tls.clone(),
        config.s2s.map(|c| c.address),
    );

    // Start S2S heartbeat
    matrix
        .sync_manager
        .start_heartbeat(matrix.lifecycle_manager.shutdown_tx.subscribe());

    gateway.run().await?;

    info!("Gateway stopped, waiting for tasks to finish...");
    // Give tasks a moment to flush buffers and close connections
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    Ok(())
}
