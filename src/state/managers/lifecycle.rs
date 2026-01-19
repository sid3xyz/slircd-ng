use crate::state::Uid;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Manages server lifecycle events (shutdown, user disconnects, background tasks).
pub struct LifecycleManager {
    /// Signal for server shutdown (broadcast to all tasks).
    pub shutdown_tx: broadcast::Sender<()>,

    /// Channel for requesting user disconnects (from async contexts).
    pub disconnect_tx: mpsc::Sender<(Uid, String)>,
}

impl LifecycleManager {
    /// Create a new LifecycleManager.
    pub fn new(disconnect_tx: mpsc::Sender<(Uid, String)>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_tx,
            disconnect_tx,
        }
    }

    /// Request that a user be disconnected.
    pub fn request_disconnect(&self, uid: &str, reason: &str) {
        let _ = self
            .disconnect_tx
            .try_send((uid.to_string(), reason.to_string()));
    }

    /// Spawn all background maintenance tasks.
    pub fn spawn_background_tasks(&self, matrix: Arc<crate::state::Matrix>) {
        // Spawn signal handler for graceful shutdown
        {
            let shutdown_tx = self.shutdown_tx.clone();
            tokio::spawn(async move {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
                let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

                tokio::select! {
                    _ = sigint.recv() => tracing::info!("Received SIGINT - initiating graceful shutdown"),
                    _ = sigterm.recv() => tracing::info!("Received SIGTERM - initiating graceful shutdown"),
                }

                // Broadcast shutdown signal to all tasks
                let _ = shutdown_tx.send(());
            });
        }
        
        // Spawn channel persistence restoration
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let repo = crate::state::persistence::ChannelStateRepository::new(matrix.db.pool());
                match repo.load_all().await {
                    Ok(states) => {
                        if !states.is_empty() {
                            tracing::info!(count = states.len(), "Restoring persistent channels");
                            matrix
                                .channel_manager
                                .restore(states, Arc::downgrade(&matrix))
                                .await;
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to load channel states for restoration");
                    }
                }
            });
        }

        // Spawn periodic channel persistence sync task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 5 minutes
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    interval.tick().await;
                    matrix.channel_manager.sync_all_channels().await;
                }
            });
        }

        // Always-on writeback task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let written = matrix.client_manager.writeback_dirty().await;
                            if written > 0 {
                                tracing::debug!(count = written, "Always-on writeback completed");
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            tracing::info!("Always-on writeback task stopping");
                            let written = matrix.client_manager.writeback_dirty().await;
                            tracing::info!(count = written, "Final always-on writeback completed");
                            break;
                        }
                    }
                }
            });
        }
        
        // Nick enforcement task
        crate::services::enforce::spawn_enforcement_task(Arc::clone(&matrix));

        // WHOWAS cleanup task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            matrix.user_manager.cleanup_whowas(7);
                            tracing::info!("WHOWAS cleanup completed");
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        }

        // Shun expiry cleanup task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                loop {
                    tokio::select! {
                         _ = interval.tick() => {
                            let now = chrono::Utc::now().timestamp();
                            let before = matrix.security_manager.shuns.len();
                            matrix
                                .security_manager
                                .shuns
                                .retain(|_, shun| shun.expires_at.is_none_or(|exp| exp > now));
                            let removed = before - matrix.security_manager.shuns.len();
                            if removed > 0 {
                                tracing::info!(removed = removed, "Expired shuns removed");
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        }
        
        // Always-on expiration task
        {
            let matrix = Arc::clone(&matrix);
            let expiration = matrix.config.multiclient.parse_expiration();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Some(expiration) = expiration {
                                let expired_memory = matrix.client_manager.expire_clients(expiration).await;
                                if !expired_memory.is_empty() {
                                    tracing::info!(
                                        count = expired_memory.len(),
                                        "Expired stale always-on clients from memory"
                                    );
                                }

                                let cutoff = chrono::Utc::now() - expiration;
                                match matrix.client_manager.expire_from_storage(cutoff) {
                                    Ok(expired_storage) if !expired_storage.is_empty() => {
                                        tracing::info!(
                                            count = expired_storage.len(),
                                            "Expired stale always-on clients from storage"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(error = %e, "Failed to expire clients from storage");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        }
        
        // Ban cache and rate limiter pruning task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let removed = matrix.security_manager.ban_cache.prune_expired();
                            if removed > 0 {
                                tracing::info!(removed = removed, "Expired bans pruned from cache");
                            }
                            if let Ok(mut deny_list) = matrix.security_manager.ip_deny_list.write() {
                                let pruned = deny_list.prune_expired();
                                if pruned > 0 {
                                    tracing::info!(removed = pruned, "Expired IP deny entries pruned");
                                }
                            }
                            matrix.security_manager.rate_limiter.cleanup();
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        }

        // Message history pruning task
        {
            let matrix = Arc::clone(&matrix);
            tokio::spawn(async move {
                let retention = std::time::Duration::from_secs(30 * 86400);
                let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
                
                // Run immediately at startup
                match matrix.service_manager.history.prune(retention).await {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(
                            removed = removed,
                            "Startup: Old messages pruned from history"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "Startup: Failed to prune message history");
                    }
                }

                // Then run daily
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(86400));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match matrix.service_manager.history.prune(retention).await {
                                Ok(removed) if removed > 0 => {
                                    tracing::info!(removed = removed, "Old messages pruned from history");
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to prune message history");
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                    }
                }
            });
        }
    }
}