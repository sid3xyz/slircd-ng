//! Actor Model for Channel State Management.
//!
//! This module implements the `ChannelActor`, which manages the state of a single IRC channel
//! in an isolated Tokio task. This eliminates `RwLock` contention on the hot path (message routing).
//!
//! # Architecture
//!
//! - **State Ownership**: The `ChannelActor` owns all channel state (members, modes, topic, bans).
//! - **Message Passing**: All interactions happen via `ChannelEvent` messages sent to the actor.
//! - **Concurrency**: Each channel runs on its own task, allowing the runtime to distribute load.

use crate::state::observer::StateObserver;
use crate::state::{ListEntry, Matrix, MemberModes, Topic};
use chrono::Utc;
use slirc_proto::Message;
use slirc_proto::sync::clock::HybridTimestamp;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

mod crdt;
mod handlers;
mod helpers;
mod types;
pub mod validation;

pub use helpers::modes_to_string;
pub use types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorState {
    Active,
    Draining,
}

/// The Channel Actor.
///
/// Owns the state of a single channel and processes events sequentially.
pub struct ChannelActor {
    pub name: String,
    pub members: im::HashMap<Uid, MemberModes>,
    pub user_nicks: HashMap<Uid, String>,
    pub senders: HashMap<Uid, mpsc::Sender<Arc<Message>>>,
    pub user_caps: HashMap<Uid, HashSet<String>>,
    pub modes: HashSet<ChannelMode>,
    /// Timestamps for when each boolean mode was last changed.
    /// Key is the mode character (e.g., 'n' for NoExternal).
    pub mode_timestamps: HashMap<char, HybridTimestamp>,
    /// Timestamp for the topic.
    pub topic_timestamp: Option<HybridTimestamp>,
    /// Server ID for generating hybrid timestamps.
    pub server_id: slirc_proto::sync::ServerId,
    /// Channel metadata (Ergo extension)
    pub metadata: HashMap<String, String>,
    pub topic: Option<Topic>,
    pub created: i64,

    // Lists
    pub bans: Vec<ListEntry>,
    pub excepts: Vec<ListEntry>,
    pub invex: Vec<ListEntry>,
    pub quiets: Vec<ListEntry>,
    /// Users who joined via +D (Delayed Join) and haven't spoken yet.
    pub silent_members: HashSet<Uid>,

    // State
    pub invites: VecDeque<InviteEntry>,
    pub kicked_users: HashMap<Uid, Instant>,
    /// Flood protection config (by type)
    pub flood_config: HashMap<FloodType, FloodParam>,
    /// Per-user message flood limiters for this channel
    pub flood_message_limiters: HashMap<Uid, governor::DefaultDirectRateLimiter>,
    /// Channel-wide join limiter for 'j' mode
    pub flood_join_limiter: Option<governor::DefaultDirectRateLimiter>,
    matrix: Weak<Matrix>,
    state: ActorState,
    observer: Option<Arc<dyn StateObserver>>,
}

const MAX_INVITES_PER_CHANNEL: usize = 100;
const INVITE_TTL: Duration = Duration::from_secs(60 * 60); // 1 hour

impl ChannelActor {
    fn request_disconnect(&self, uid: &Uid, reason: &str) {
        if let Some(matrix) = self.matrix.upgrade() {
            matrix.request_disconnect(uid, reason);
        }
    }

    /// Create a new Channel Actor with custom mailbox capacity.
    /// The capacity controls how many events can be queued before senders block.
    /// Higher values provide burst tolerance; lower values apply backpressure sooner.
    pub fn spawn_with_capacity(
        name: String,
        matrix: Weak<Matrix>,
        initial_topic: Option<Topic>,
        capacity: usize,
        observer: Option<Arc<dyn StateObserver>>,
    ) -> mpsc::Sender<ChannelEvent> {
        let (tx, rx) = mpsc::channel(capacity);

        // Default channel modes: +nt (NoExternal, TopicLock)
        let mut modes = HashSet::with_capacity(8); // Typical channel mode count
        modes.insert(ChannelMode::NoExternal);
        modes.insert(ChannelMode::TopicLock);

        // Get server_id from matrix (use default if matrix unavailable - shouldn't happen)
        let server_id = matrix
            .upgrade()
            .map(|m| m.server_id.clone())
            .unwrap_or_else(|| slirc_proto::sync::ServerId::new("000".to_string()));

        let actor = Self {
            name,
            members: im::HashMap::new(),
            user_nicks: HashMap::new(),
            senders: HashMap::new(),
            user_caps: HashMap::new(),
            modes,
            mode_timestamps: HashMap::new(),
            topic_timestamp: None,
            server_id,
            metadata: HashMap::new(),
            topic: initial_topic,
            created: Utc::now().timestamp(),
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
            invites: VecDeque::new(),
            silent_members: HashSet::new(),
            kicked_users: HashMap::new(),
            flood_config: HashMap::new(),
            flood_message_limiters: HashMap::new(),
            flood_join_limiter: None,
            matrix,
            state: ActorState::Active,
            observer,
        };

        tokio::spawn(async move {
            actor.run(rx).await;
        });
        tx
    }

    /// The main actor loop.
    pub async fn run(mut self, mut rx: mpsc::Receiver<ChannelEvent>) {
        while let Some(event) = rx.recv().await {
            self.handle_event(event).await;
        }
    }

    async fn handle_event(&mut self, event: ChannelEvent) {
        match event {
            ChannelEvent::Join { params, reply_tx } => {
                self.handle_join(*params, reply_tx).await;
            }
            ChannelEvent::Part {
                uid,
                reason,
                prefix,
                reply_tx,
            } => {
                self.handle_part(uid, reason, prefix, reply_tx).await;
            }
            ChannelEvent::Quit {
                uid,
                quit_msg,
                reply_tx,
            } => {
                self.handle_quit(uid, quit_msg, reply_tx).await;
            }
            ChannelEvent::Detach { uid, reply_tx } => {
                if self.members.contains_key(&uid) {
                    self.members.remove(&uid);
                    self.senders.remove(&uid);
                    self.user_nicks.remove(&uid);
                    self.user_caps.remove(&uid);
                    crate::metrics::set_channel_members(&self.name, self.members.len() as i64);
                    self.cleanup_if_empty();
                }
                let _ = reply_tx.send(self.members.len());
            }
            ChannelEvent::Message { params, reply_tx } => {
                self.handle_message(*params, reply_tx).await;
            }
            ChannelEvent::Broadcast { message, exclude } => {
                self.handle_broadcast(message, exclude).await;
            }
            ChannelEvent::BroadcastWithCap {
                message,
                exclude,
                required_cap,
                fallback_msg,
            } => {
                self.handle_broadcast_with_cap(
                    *message,
                    exclude,
                    required_cap,
                    fallback_msg.map(|m| *m),
                )
                .await;
            }
            ChannelEvent::UpdateCaps { uid, caps } => {
                if self.members.contains_key(&uid) {
                    self.user_caps.insert(uid, caps);
                }
            }
            ChannelEvent::GetInfo {
                requester_uid,
                reply_tx,
            } => {
                let is_member = if let Some(uid) = requester_uid {
                    self.members.contains_key(&uid)
                } else {
                    false
                };
                let info = ChannelInfo {
                    name: self.name.clone(),
                    topic: self.topic.clone(),
                    member_count: self.members.len(),
                    created: self.created,
                    modes: self.modes.clone(),
                    is_member,
                    members: self.members.keys().cloned().collect(),
                };
                let _ = reply_tx.send(info);
            }
            ChannelEvent::MergeCrdt { crdt, source } => {
                self.handle_merge_crdt(*crdt, source).await;
            }
            ChannelEvent::GetList { mode, reply_tx } => {
                let list = match mode {
                    'b' => self.bans.clone(),
                    'e' => self.excepts.clone(),
                    'I' => self.invex.clone(),
                    'q' => self.quiets.clone(),
                    _ => Vec::new(),
                };
                let _ = reply_tx.send(list);
            }
            ChannelEvent::GetMembers { reply_tx } => {
                let _ = reply_tx.send(self.members.clone());
            }
            ChannelEvent::GetMemberModes { uid, reply_tx } => {
                let modes = self.members.get(&uid).cloned();
                let _ = reply_tx.send(modes);
            }
            ChannelEvent::GetModes { reply_tx } => {
                let _ = reply_tx.send(self.modes.clone());
            }
            ChannelEvent::ApplyModes { params, reply_tx } => {
                self.handle_apply_modes(params, reply_tx).await;
            }
            ChannelEvent::Kick { params, reply_tx } => {
                self.handle_kick(params, reply_tx).await;
            }
            ChannelEvent::SetTopic { params, reply_tx } => {
                self.handle_set_topic(params, reply_tx).await;
            }
            ChannelEvent::Invite { params, reply_tx } => {
                self.handle_invite(params, reply_tx).await;
            }
            ChannelEvent::Knock {
                sender_uid,
                sender_prefix,
                reply_tx,
            } => {
                self.handle_knock(sender_uid, sender_prefix, reply_tx).await;
            }
            ChannelEvent::NickChange { uid, new_nick } => {
                self.handle_nick_change(uid, new_nick).await;
            }
            ChannelEvent::Clear {
                sender_uid,
                sender_prefix,
                target,
                reply_tx,
            } => {
                self.handle_clear(sender_uid, sender_prefix, target, reply_tx)
                    .await;
            }
            ChannelEvent::RemoteMode {
                ts,
                setter,
                modes,
                args,
            } => {
                self.handle_remote_mode(ts, setter, modes, args).await;
            }
            ChannelEvent::RemoteTopic { ts, setter, topic } => {
                self.handle_remote_topic(ts, setter, topic).await;
            }
            ChannelEvent::RemoteKick {
                sender,
                target,
                reason,
            } => {
                self.handle_remote_kick(sender, target, Some(reason)).await;
            }
            ChannelEvent::NetsplitQuit { uid, reply_tx } => {
                // Silently remove user from channel (QUIT already broadcast by split.rs)
                self.members.remove(&uid);
                self.senders.remove(&uid);
                self.user_nicks.remove(&uid);
                self.user_caps.remove(&uid);
                let _ = reply_tx.send(());
            }
            ChannelEvent::Metadata { command, reply_tx } => {
                let _ = reply_tx.send(self.handle_metadata(command));
            }
            ChannelEvent::AttachSender { uid, sender } => {
                // Multiclient: attach a new session's sender to existing member
                if self.members.contains_key(&uid) {
                    self.senders.insert(uid, sender);
                }
            }
        }
    }

    fn cleanup_if_empty(&mut self) {
        if self.state == ActorState::Draining {
            return;
        }

        let is_permanent = self.modes.contains(&ChannelMode::Permanent);
        if self.members.is_empty() && !is_permanent {
            self.state = ActorState::Draining;

            // Remove channel member metrics (Innovation 3)
            crate::metrics::remove_channel_metrics(&self.name);

            if let Some(observer) = &self.observer {
                observer.on_channel_destroy(&self.name, None);
            }

            if let Some(matrix) = self.matrix.upgrade() {
                let name_lower = self.name.to_lowercase();
                if matrix
                    .channel_manager
                    .channels
                    .remove(&name_lower)
                    .is_some()
                {
                    crate::metrics::ACTIVE_CHANNELS.dec();
                }
            }
        }
    }

    async fn handle_nick_change(&mut self, uid: Uid, new_nick: String) {
        if self.user_nicks.contains_key(&uid) {
            self.user_nicks.insert(uid, new_nick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::Weak;

    fn create_test_channel_actor() -> ChannelActor {
        ChannelActor {
            name: "#test".to_string(),
            members: im::HashMap::new(),
            user_nicks: HashMap::new(),
            senders: HashMap::new(),
            user_caps: HashMap::new(),
            modes: HashSet::new(),
            mode_timestamps: HashMap::new(),
            topic_timestamp: None,
            server_id: slirc_proto::sync::ServerId::new("000".to_string()),
            topic: None,
            created: 0,
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
            invites: VecDeque::new(),
            silent_members: HashSet::new(),
            kicked_users: HashMap::new(),
            flood_config: HashMap::new(),
            flood_message_limiters: HashMap::new(),
            flood_join_limiter: None,
            matrix: Weak::new(),
            state: ActorState::Active,
            observer: None,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_nick_change_updates_user_nicks() {
        let mut actor = create_test_channel_actor();

        let uid = "user123".to_string();
        let old_nick = "oldnick".to_string();
        let new_nick = "newnick".to_string();

        actor.user_nicks.insert(uid.clone(), old_nick.clone());
        assert_eq!(actor.user_nicks.get(&uid), Some(&old_nick));

        actor
            .handle_nick_change(uid.clone(), new_nick.clone())
            .await;
        assert_eq!(actor.user_nicks.get(&uid), Some(&new_nick));
    }

    #[tokio::test]
    async fn test_nick_change_ignores_non_member() {
        let mut actor = create_test_channel_actor();

        let uid = "user123".to_string();
        let new_nick = "newnick".to_string();

        assert_eq!(actor.user_nicks.get(&uid), None);

        actor
            .handle_nick_change(uid.clone(), new_nick.clone())
            .await;
        assert_eq!(actor.user_nicks.get(&uid), None);
    }
}
