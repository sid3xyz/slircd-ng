//! Automatic configuration file watching and hot-reload
//! RUST ARCHITECT: Optional development feature, SIGHUP recommended for explicit reload control
//!
//! This module provides automatic file-watching for config.toml changes.
//! When enabled, configuration changes are automatically reloaded without
//! requiring manual SIGHUP signals. This is convenient for development.

use crate::core::state::ServerState;
use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Start watching configuration file for changes
/// RUST ARCHITECT: Spawns async task that monitors file and triggers reloads
pub fn start_watching(
    config_path: impl AsRef<Path>,
    state: Arc<ServerState>,
    debounce_ms: u64,
) -> Result<()> {
    let config_path = config_path.as_ref().to_path_buf();
    let canonical_path = tokio::task::block_in_place(|| {
        config_path
            .canonicalize()
    })
    .context("failed to canonicalize config path")?;

    info!(?canonical_path, debounce_ms, "starting config file watcher");

    // Create channel for file system events
    let (tx, mut rx) = mpsc::channel(100);

    // Spawn file watcher in separate thread (notify uses std::sync channels)
    std::thread::spawn(move || {
        // Create a new Tokio runtime for this thread since notify requires std::sync
        let rt = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(e) => {
                error!(error = %e, "failed to create tokio runtime for config watcher");
                return;
            }
        };

        let mut watcher: RecommendedWatcher = match Watcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only care about modify events
                        if matches!(event.kind, EventKind::Modify(_)) {
                            let _ = rt.block_on(tx.send(event));
                        }
                    }
                    Err(e) => error!(error = %e, "file watcher error"),
                }
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                error!(error = %e, "failed to create file watcher");
                return;
            }
        };

        // Watch the config file
        if let Err(e) = watcher.watch(&canonical_path, RecursiveMode::NonRecursive) {
            error!(error = %e, ?canonical_path, "failed to watch config file");
            return;
        }

        info!("file watcher thread started");

        // Keep watcher alive by blocking forever
        // The watcher will be dropped when the program exits
        std::thread::park();
    });

    // Spawn async task to handle reload events
    tokio::spawn(async move {
        let mut last_reload = std::time::Instant::now();
        let debounce = Duration::from_millis(debounce_ms);

        while let Some(_event) = rx.recv().await {
            // Debounce: ignore events within debounce_ms of last reload
            let elapsed = last_reload.elapsed();
            if elapsed < debounce {
                continue;
            }

            info!("config file changed, reloading");
            last_reload = std::time::Instant::now();

            // Trigger reload (handle non-UTF8 paths gracefully)
            let config_path_str = config_path.to_string_lossy();
            match state.reload_config(&config_path_str).await {
                Ok(()) => {
                    info!("configuration reloaded successfully via file watcher");
                }
                Err(e) => {
                    error!(error = %e, "failed to reload configuration");
                    warn!("keeping old configuration due to reload failure");
                }
            }
        }

        info!("file watcher task terminated");
    });

    Ok(())
}
