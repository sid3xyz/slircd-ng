//! Channel management state.
//!
//! This module contains the `ChannelManager` struct, which isolates all
//! channel-related state from the main Matrix struct.

use crate::state::actor::{ChannelEvent, ChannelInfo};
use crate::state::observer::StateObserver;
use dashmap::{DashMap, DashSet};
use futures_util::future::join_all;
use slirc_proto::Message;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Channel management state and behavior.
///
/// The ChannelManager is responsible for:
/// - Tracking active channel actors.
/// - Managing registered channel names.
/// - Providing high-level broadcasting primitives.
pub struct ChannelManager {
    /// All channels, indexed by lowercase name.
    /// Each channel has an actor (mpsc::Sender) that processes ChannelEvents.
    pub channels: DashMap<String, mpsc::Sender<ChannelEvent>>,

    /// Set of registered channel names (lowercase) for fast lookup.
    /// These are channels registered with ChanServ.
    pub registered_channels: DashSet<String>,

    /// Observer for state changes (Innovation 2).
    pub observer: Option<Arc<dyn StateObserver>>,

    /// Usage stats manager.
    pub stats_manager: Arc<crate::state::managers::stats::StatsManager>,
}

impl ChannelManager {
    /// Get an existing channel actor or create a new one.
    pub async fn get_or_create_actor(
        &self,
        name: String,
        matrix: std::sync::Weak<crate::state::Matrix>,
    ) -> mpsc::Sender<ChannelEvent> {
        let name_lower = name.to_lowercase();
        if let Some(tx) = self.channels.get(&name_lower) {
            tx.clone()
        } else {
            use crate::state::actor::ChannelActor;
            let tx = ChannelActor::spawn_with_capacity(
                name,
                matrix,
                None, // initial_topic
                None, // initial_modes
                None, // created_at
                100,  // capacity
                self.observer.clone(),
            );
            self.channels.insert(name_lower, tx.clone());
            crate::metrics::inc_active_channels();
            self.stats_manager.channel_created();
            tx
        }
    }

    /// Restore channels from persistent state.
    pub async fn restore(
        &self,
        states: Vec<crate::state::persistence::ChannelState>,
        matrix: std::sync::Weak<crate::state::Matrix>,
    ) {
        use crate::state::Topic;
        use crate::state::actor::ChannelActor;
        use crate::state::actor::modes_from_string;

        for state in states {
            let name = state.name.clone();
            let name_lower = name.to_lowercase();

            let initial_topic = if let (Some(text), Some(set_by), Some(set_at)) =
                (state.topic, state.topic_set_by, state.topic_set_at)
            {
                Some(Topic {
                    text,
                    set_by,
                    set_at,
                })
            } else {
                None
            };

            let initial_modes = Some(modes_from_string(&state.modes, state.key, state.user_limit));

            let tx = ChannelActor::spawn_with_capacity(
                name,
                matrix.clone(),
                initial_topic,
                initial_modes,
                Some(state.created_at),
                100,
                self.observer.clone(),
            );

            self.channels.insert(name_lower, tx);
            crate::metrics::inc_active_channels();
            self.stats_manager.channel_created();
        }
    }

    /// Trigger persistence sync for all active channels.
    pub async fn sync_all_channels(&self) {
        let channels: Vec<_> = self
            .channels
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        for tx in channels {
            let _ = tx.send(ChannelEvent::CheckAndSave).await;
        }
    }

    /// Initialize with pre-loaded registered channels.
    pub fn with_registered_channels(
        registered_channels: Vec<String>,
        stats_manager: Arc<crate::state::managers::stats::StatsManager>,
    ) -> Self {
        let registered_set = DashSet::with_capacity(registered_channels.len());
        for name in registered_channels {
            registered_set.insert(name);
        }

        Self {
            channels: DashMap::new(),
            registered_channels: registered_set,
            observer: None,
            stats_manager,
        }
    }

    /// Set the state observer.
    pub fn set_observer(&mut self, observer: Arc<dyn StateObserver>) {
        self.observer = Some(observer);
    }

    /// Optionally exclude one UID (usually the sender).
    /// Note: `channel_name` should already be lowercased by the caller.
    ///
    /// Uses `Arc<Message>` for efficient broadcasting to multiple recipients.
    pub async fn broadcast_to_channel(
        &self,
        channel_name: &str,
        msg: Message,
        exclude: Option<&str>,
    ) {
        let channel_tx = self.channels.get(channel_name).map(|s| s.value().clone());
        if let Some(channel_tx) = channel_tx {
            let _ = channel_tx
                .send(ChannelEvent::Broadcast {
                    message: msg,
                    exclude: exclude.map(|s| s.to_string()),
                })
                .await;
        }
    }

    /// Broadcast to channel members filtered by IRCv3 capability.
    ///
    /// - If `required_cap` is Some, only sends `msg` to members who have that capability enabled
    /// - If `fallback_msg` is Some, sends that to members without the capability
    /// - If `fallback_msg` is None, members without the capability receive nothing
    pub async fn broadcast_to_channel_with_cap(
        &self,
        channel_name: &str,
        msg: Message,
        exclude: Option<&str>,
        required_cap: Option<&str>,
        fallback_msg: Option<Message>,
    ) {
        let excludes: &[&str] = if let Some(e) = exclude { &[e] } else { &[] };
        self.broadcast_to_channel_with_cap_exclude_users(
            channel_name,
            msg,
            excludes,
            required_cap,
            fallback_msg,
        )
        .await
    }

    /// Broadcast a message to channel members, filtering by capability and excluding multiple users.
    ///
    /// Same as `broadcast_to_channel_with_cap` but allows excluding multiple users.
    pub async fn broadcast_to_channel_with_cap_exclude_users(
        &self,
        channel_name: &str,
        msg: Message,
        exclude: &[&str],
        required_cap: Option<&str>,
        fallback_msg: Option<Message>,
    ) {
        let channel_tx = self.channels.get(channel_name).map(|s| s.value().clone());
        if let Some(channel_tx) = channel_tx {
            let _ = channel_tx
                .send(ChannelEvent::BroadcastWithCap {
                    message: Box::new(msg),
                    exclude: exclude.iter().map(|s| s.to_string()).collect(),
                    required_cap: required_cap.map(|s| s.to_string()),
                    fallback_msg: fallback_msg.map(Box::new),
                })
                .await;
        }
    }

    /// Get information for all channels concurrently.
    ///
    /// This method:
    /// 1. Collects all channel senders from the DashMap
    /// 2. Spawns concurrent tasks to send GetInfo events to each channel actor
    /// 3. Uses `futures::future::join_all` to await all responses concurrently
    /// 4. Filters out failed responses and returns the collected ChannelInfo structs
    ///
    /// # Arguments
    /// * `requester_uid` - Optional UID of the user requesting the info (for membership checks)
    ///
    /// # Returns
    /// Vector of `ChannelInfo` structs for all successfully queried channels
    pub async fn get_all_channel_info(&self, requester_uid: Option<String>) -> Vec<ChannelInfo> {
        // Collect all channel senders from the DashMap
        let channel_senders: Vec<_> = self
            .channels
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        // Spawn concurrent tasks to query each channel
        let tasks: Vec<_> = channel_senders
            .into_iter()
            .map(|channel_tx| {
                let requester_uid = requester_uid.clone();
                async move {
                    let (reply_tx, reply_rx) = oneshot::channel();

                    // Send GetInfo event to channel actor
                    if channel_tx
                        .send(ChannelEvent::GetInfo {
                            requester_uid,
                            reply_tx,
                        })
                        .await
                        .is_err()
                    {
                        return None;
                    }

                    // Await response from channel actor
                    reply_rx.await.ok()
                }
            })
            .collect();

        // Wait for all tasks to complete concurrently
        let results = join_all(tasks).await;

        // Filter out failed responses and collect successful ones
        results.into_iter().flatten().collect()
    }
}
