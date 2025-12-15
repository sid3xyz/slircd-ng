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

use crate::state::{ListEntry, Matrix, MemberModes, Topic};
use chrono::Utc;
use slirc_proto::Message;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Weak;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

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
    pub senders: HashMap<Uid, mpsc::Sender<Message>>,
    pub user_caps: HashMap<Uid, HashSet<String>>,
    pub modes: HashSet<ChannelMode>,
    pub topic: Option<Topic>,
    pub created: i64,

    // Lists
    pub bans: Vec<ListEntry>,
    pub excepts: Vec<ListEntry>,
    pub invex: Vec<ListEntry>,
    pub quiets: Vec<ListEntry>,

    // State
    pub invites: VecDeque<InviteEntry>,
    pub kicked_users: HashMap<Uid, Instant>,
    matrix: Weak<Matrix>,
    state: ActorState,
}

const MAX_INVITES_PER_CHANNEL: usize = 100;
const INVITE_TTL: Duration = Duration::from_secs(60 * 60); // 1 hour

impl ChannelActor {
    fn request_disconnect(&self, uid: &Uid, reason: &str) {
        if let Some(matrix) = self.matrix.upgrade() {
            matrix.request_disconnect(uid, reason);
        }
    }

    /// Create a new Channel Actor and spawn it.
    /// Optionally pass an initial topic for registered channels (loaded from DB).
    pub fn spawn(name: String, matrix: Weak<Matrix>, initial_topic: Option<Topic>) -> mpsc::Sender<ChannelEvent> {
        let (tx, rx) = mpsc::channel(100);

        let mut modes = HashSet::new();
        modes.insert(ChannelMode::NoExternal);
        modes.insert(ChannelMode::TopicLock);

        let actor = Self {
            name,
            members: im::HashMap::new(),
            user_nicks: HashMap::new(),
            senders: HashMap::new(),
            user_caps: HashMap::new(),
            modes,
            topic: initial_topic,
            created: Utc::now().timestamp(),
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
            invites: VecDeque::new(),
            kicked_users: HashMap::new(),
            matrix,
            state: ActorState::Active,
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
            ChannelEvent::Join {
                uid,
                nick,
                sender,
                caps,
                user_context,
                key,
                initial_modes,
                join_msg_extended,
                join_msg_standard,
                session_id,
                reply_tx,
            } => {
                self.handle_join(
                    uid,
                    nick,
                    sender,
                    caps,
                    *user_context,
                    key,
                    initial_modes,
                    *join_msg_extended,
                    *join_msg_standard,
                    session_id,
                    reply_tx,
                )
                .await;
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
            ChannelEvent::Message {
                sender_uid,
                text,
                tags,
                is_notice,
                is_tagmsg,
                user_context,
                is_registered,
                is_tls,
                status_prefix,
                timestamp,
                msgid,
                reply_tx,
            } => {
                self.handle_message(
                    sender_uid,
                    text,
                    tags,
                    is_notice,
                    is_tagmsg,
                    *user_context,
                    is_registered,
                    is_tls,
                    status_prefix,
                    timestamp,
                    msgid,
                    reply_tx,
                )
                .await;
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
            ChannelEvent::ApplyModes {
                sender_uid,
                sender_prefix,
                modes,
                target_uids,
                force,
                reply_tx,
            } => {
                self.handle_apply_modes(
                    sender_uid,
                    sender_prefix,
                    modes,
                    target_uids,
                    force,
                    reply_tx,
                )
                .await;
            }
            ChannelEvent::Kick {
                sender_uid,
                sender_prefix,
                target_uid,
                target_nick,
                reason,
                force,
                reply_tx,
            } => {
                self.handle_kick(
                    sender_uid,
                    sender_prefix,
                    target_uid,
                    target_nick,
                    reason,
                    force,
                    reply_tx,
                )
                .await;
            }
            ChannelEvent::SetTopic {
                sender_uid,
                sender_prefix,
                topic,
                msgid,
                timestamp,
                force,
                reply_tx,
            } => {
                self.handle_set_topic(sender_uid, sender_prefix, topic, msgid, timestamp, force, reply_tx)
                    .await;
            }
            ChannelEvent::Invite {
                sender_uid,
                sender_prefix,
                target_uid,
                target_nick,
                force,
                reply_tx,
            } => {
                self.handle_invite(
                    sender_uid,
                    sender_prefix,
                    target_uid,
                    target_nick,
                    force,
                    reply_tx,
                )
                .await;
            }
            ChannelEvent::Knock {
                sender_uid,
                sender_prefix,
                reply_tx,
            } => {
                self.handle_knock(sender_uid, sender_prefix, reply_tx).await;
            }
            ChannelEvent::NickChange {
                uid,
                old_nick: _,
                new_nick,
            } => {
                self.handle_nick_change(uid, new_nick).await;
            }            ChannelEvent::Clear {
                sender_uid,
                sender_prefix,
                target,
                reply_tx,
            } => {
                self.handle_clear(sender_uid, sender_prefix, target, reply_tx)
                    .await;
            }        }
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

            if let Some(matrix) = self.matrix.upgrade() {
                let name_lower = self.name.to_lowercase();
                if matrix.channels.remove(&name_lower).is_some() {
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
            topic: None,
            created: 0,
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
            invites: VecDeque::new(),
            kicked_users: HashMap::new(),
            matrix: Weak::new(),
            state: ActorState::Active,
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
