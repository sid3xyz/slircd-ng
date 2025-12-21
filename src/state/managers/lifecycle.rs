//! Lifecycle management state and behavior.
//!
//! This module contains the `LifecycleManager` struct, which isolates
//! server lifecycle events (shutdown, disconnects) from the main Matrix struct.

use crate::state::Uid;
use tokio::sync::{broadcast, mpsc};

/// Lifecycle management state.
///
/// The LifecycleManager handles:
/// - Server shutdown signaling
/// - User disconnect requests
pub struct LifecycleManager {
    /// Shutdown signal broadcaster.
    /// When DIE command is issued, a message is sent on this channel.
    pub shutdown_tx: broadcast::Sender<()>,

    /// Disconnect request channel.
    /// Channel actors use this to request disconnects without blocking.
    /// Bounded channel provides backpressure to prevent memory exhaustion.
    pub disconnect_tx: mpsc::Sender<(Uid, String)>,
}

impl LifecycleManager {
    /// Create a new LifecycleManager.
    pub fn new(disconnect_tx: mpsc::Sender<(Uid, String)>) -> Self {
        // Create shutdown broadcast channel
        // Capacity 16 provides buffer for multiple slow subscribers during shutdown
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            shutdown_tx,
            disconnect_tx,
        }
    }

    /// Request a user disconnect.
    ///
    /// This sends a request to the disconnect worker to disconnect the given user.
    /// The operation is non-blocking and may drop the request if the channel is full.
    pub fn request_disconnect(&self, uid: &str, reason: &str) {
        // Use try_send to maintain non-blocking behavior with bounded channel.
        // If the channel is full, the disconnect request is dropped - this is
        // acceptable as it indicates the disconnect worker is overwhelmed and
        // will catch up.
        let _ = self
            .disconnect_tx
            .try_send((uid.to_string(), reason.to_string()));
    }
}
