//! Channel management state.
//!
//! This module contains the `ChannelManager` struct, which isolates all
//! channel-related state from the main Matrix struct.

use crate::state::actor::ChannelEvent;
use crate::state::observer::StateObserver;
use dashmap::{DashMap, DashSet};
use slirc_proto::Message;
use std::sync::Arc;
use tokio::sync::mpsc;

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
}

impl ChannelManager {
    /// Create a new ChannelManager.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            registered_channels: DashSet::new(),
            observer: None,
        }
    }

    /// Get an existing channel actor or create a new one.
    #[allow(dead_code)] // Reserved for S2S SJOIN handling
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
            let tx =
                ChannelActor::spawn_with_capacity(name, matrix, None, 100, self.observer.clone());
            self.channels.insert(name_lower, tx.clone());
            crate::metrics::ACTIVE_CHANNELS.inc();
            tx
        }
    }

    /// Initialize with pre-loaded registered channels.
    pub fn with_registered_channels(registered_channels: Vec<String>) -> Self {
        let registered_set = DashSet::with_capacity(registered_channels.len());
        for name in registered_channels {
            registered_set.insert(name);
        }

        Self {
            channels: DashMap::new(),
            registered_channels: registered_set,
            observer: None,
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

    /// Export all channels as CRDTs for a BURST.
    #[allow(dead_code)]
    pub async fn to_crdt(&self) -> Vec<slirc_crdt::channel::ChannelCrdt> {
        use crate::state::actor::ChannelEvent;
        let mut crdts = Vec::new();
        for entry in self.channels.iter() {
            let channel_tx = entry.value();
            let (tx, rx) = tokio::sync::oneshot::channel();
            if channel_tx
                .send(ChannelEvent::GetCrdt { reply_tx: tx })
                .await
                .is_ok()
                && let Ok(crdt) = rx.await
            {
                crdts.push(crdt);
            }
        }
        crdts
    }

    /// Merge a ChannelCrdt into the local state.
    #[allow(dead_code)] // Reserved for S2S CRDT sync
    pub async fn merge_channel_crdt(
        &self,
        crdt: slirc_crdt::channel::ChannelCrdt,
        matrix: std::sync::Weak<crate::state::Matrix>,
        source: Option<slirc_crdt::clock::ServerId>,
    ) {
        use crate::state::actor::{ChannelActor, ChannelEvent};
        let name_lower = crdt.name.to_lowercase();

        if let Some(channel_tx) = self.channels.get(&name_lower) {
            let _ = channel_tx
                .send(ChannelEvent::MergeCrdt {
                    crdt: Box::new(crdt),
                    source,
                })
                .await;
        } else {
            let tx = ChannelActor::spawn_with_capacity(
                crdt.name.clone(),
                matrix,
                None,
                100,
                self.observer.clone(),
            );
            let _ = tx
                .send(ChannelEvent::MergeCrdt {
                    crdt: Box::new(crdt),
                    source,
                })
                .await;
            self.channels.insert(name_lower, tx);
            crate::metrics::ACTIVE_CHANNELS.inc();
        }
    }
}
