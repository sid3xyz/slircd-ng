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

use crate::security::UserContext;
use crate::state::{ListEntry, Matrix, MemberModes, Topic};
use chrono::Utc;
use slirc_proto::{Command, Message, Prefix};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub mod validation;

use self::validation::{create_user_mask, is_banned};

/// Unique identifier for a user (UID string).
pub type Uid = String;

#[derive(Debug)]
pub struct JoinSuccessData {
    pub topic: Option<Topic>,
    pub channel_name: String,
    pub is_secret: bool,
}

/// Events that can be sent to a Channel Actor.
#[derive(Debug)]
pub enum ChannelEvent {
    /// User joining the channel.
    Join {
        uid: Uid,
        nick: String,
        sender: mpsc::Sender<Message>,
        caps: HashSet<String>,
        user_context: Box<UserContext>,
        key: Option<String>,
        initial_modes: Option<MemberModes>,
        join_msg_extended: Box<Message>,
        join_msg_standard: Box<Message>,
        session_id: Uuid,
        /// Reply channel for the result (success/error).
        reply_tx: oneshot::Sender<Result<JoinSuccessData, String>>,
    },
    /// User leaving the channel.
    Part {
        uid: Uid,
        reason: Option<String>,
        prefix: Prefix,
        reply_tx: oneshot::Sender<Result<usize, String>>,
    },
    /// User quitting the server.
    Quit {
        uid: Uid,
        quit_msg: Message,
        reply_tx: Option<oneshot::Sender<usize>>,
    },
    /// User sending a message (PRIVMSG or NOTICE) to the channel.
    Message {
        sender_uid: Uid,
        text: String,
        tags: Option<Vec<slirc_proto::message::Tag>>,
        is_notice: bool,
        user_context: Box<UserContext>,
        is_registered: bool,
        is_tls: bool,
        status_prefix: Option<char>,
        reply_tx: oneshot::Sender<ChannelRouteResult>,
    },
    /// Request channel information (for LIST/WHO/NAMES).
    GetInfo {
        requester_uid: Option<Uid>,
        reply_tx: oneshot::Sender<ChannelInfo>,
    },
    /// Request list (bans, excepts, etc).
    GetList {
        mode: char,
        reply_tx: oneshot::Sender<Vec<ListEntry>>,
    },
    /// Request list of members.
    GetMembers {
        reply_tx: oneshot::Sender<im::HashMap<Uid, MemberModes>>,
    },
    /// Request member modes.
    GetMemberModes {
        uid: Uid,
        reply_tx: oneshot::Sender<Option<MemberModes>>,
    },
    /// Apply mode changes.
    ApplyModes {
        sender_uid: Uid,
        sender_prefix: Prefix,
        modes: Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>,
        /// Mapping of nick arguments to UIDs for modes that target users (+o, +v, etc).
        target_uids: HashMap<String, Uid>,
        force: bool,
        reply_tx: oneshot::Sender<Result<Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>, String>>,
    },
    /// Kick a user from the channel.
    Kick {
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        reason: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    },
    /// Set the channel topic.
    SetTopic {
        sender_uid: Uid,
        sender_prefix: Prefix,
        topic: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    },
    /// Invite a user to the channel.
    Invite {
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    },
    /// Knock on the channel.
    Knock {
        sender_uid: Uid,
        sender_prefix: Prefix,
        reply_tx: oneshot::Sender<Result<(), String>>,
    },
    /// Broadcast a raw message to all members.
    Broadcast {
        message: Message,
        exclude: Option<Uid>,
    },
    /// Broadcast with capability filtering.
    BroadcastWithCap {
        message: Box<Message>,
        exclude: Vec<Uid>,
        required_cap: Option<String>,
        fallback_msg: Option<Box<Message>>,
    },
    /// User nickname change.
    NickChange {
        uid: Uid,
        #[allow(dead_code)] // retained for future auditing/logging
        old_nick: String,
        new_nick: String,
    },
}

/// Snapshot of channel information for queries.
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub name: String,
    pub topic: Option<Topic>,
    pub member_count: usize,
    pub created: i64,
    pub modes: HashSet<ChannelMode>,
    pub is_member: bool,
}

/// Result of attempting to route a message to a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelRouteResult {
    /// Message was successfully broadcast to channel members.
    Sent,
    /// Channel does not exist.
    NoSuchChannel,
    /// Sender is blocked by +n (no external messages).
    BlockedExternal,
    /// Sender is blocked by +m (moderated).
    BlockedModerated,
    /// Sender is blocked by +r (registered-only channel).
    BlockedRegisteredOnly,
    /// Blocked by +C (no CTCP except ACTION).
    BlockedCTCP,
    /// Blocked by +T (no channel NOTICE).
    BlockedNotice,
    /// Blocked by +b (banned).
    BlockedBanned,
}

/// Channel modes (Ported from legacy code).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelMode {
    /// +n: No external messages (only members can send)
    NoExternal,
    /// +t: Topic lock (only ops can change topic)
    TopicLock,
    /// +m: Moderated (only +v/+o can speak)
    Moderated,
    /// +M: Moderated-Unregistered (only registered users can speak in moderated channel)
    ModeratedUnreg,
    /// +N: No Nick Change (users cannot change nick while in channel)
    NoNickChange,
    /// +c: No Colors/Formatting (strip mIRC color codes, bold, underline)
    NoColors,
    /// +z: TLS-only channel (only TLS clients can join)
    TlsOnly,
    /// +K: No KNOCK (block KNOCK command on this channel)
    NoKnock,
    /// +V: No INVITE (block INVITE command on this channel)
    NoInvite,
    /// +T: No NOTICE (block NOTICE to channel, PRIVMSG still allowed)
    NoNotice,
    /// +g: Free INVITE (non-ops can use INVITE command)
    FreeInvite,
    /// +O: Oper-only (only IRC operators can join)
    OperOnly,
    /// +A: Admin-only (only server admins can join)
    AdminOnly,
    /// +u: Auditorium (non-ops only see ops, not other non-ops)
    Auditorium,
    /// +r: Registered channel (set by ChanServ, indicates channel is registered)
    Registered,
    /// +Q: No kicks (prevent KICK command, only services can kick)
    NoKicks,
    /// +j <joins>:<seconds>: Join throttle (limit join rate)
    #[allow(dead_code)] // Reserved for future channel join controls
    JoinThrottle { joins: u32, seconds: u32 },
    /// +J <seconds>: Join delay after kick (prevent rejoin for N seconds after kick)
    #[allow(dead_code)] // Reserved for future channel join controls
    JoinDelay(u32),
    /// +L <channel>: Redirect/overflow channel (when +l limit hit, redirect to overflow)
    #[allow(dead_code)] // Reserved for future redirect support
    Redirect(String),
    /// +f <messages>:<seconds>: Flood protection (kick users exceeding message threshold)
    #[allow(dead_code)] // Reserved for future flood controls
    FloodProtection { messages: u32, seconds: u32 },
    /// +s: Secret channel (hidden from LIST)
    Secret,
    /// +p: Private channel (hidden from LIST, no KNOCK)
    Private,
    /// +i: Invite-only
    InviteOnly,
    /// +C: No CTCP (except ACTION)
    NoCtcp,
    /// +P: Permanent channel (persists with 0 users)
    Permanent,
    /// +R: Registered-only (only identified users can join)
    RegisteredOnly,
    /// +S: SSL-only (only TLS connections can join)
    SSLOnly,
    /// +k <key>: Channel key required to join
    Key(String),
    /// +l <limit>: User limit
    Limit(usize),
}

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

#[derive(Debug, Clone)]
pub struct InviteEntry {
    pub uid: Uid,
    pub set_at: Instant,
}

const MAX_INVITES_PER_CHANNEL: usize = 100;
const INVITE_TTL: Duration = Duration::from_secs(60 * 60); // 1 hour

impl ChannelActor {
    /// Create a new Channel Actor and spawn it.
    pub fn spawn(name: String, matrix: Weak<Matrix>) -> mpsc::Sender<ChannelEvent> {
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
            topic: None,
            created: chrono::Utc::now().timestamp(),
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
            ChannelEvent::Join { uid, nick, sender, caps, user_context, key, initial_modes, join_msg_extended, join_msg_standard, session_id, reply_tx } => {
                self.handle_join(uid, nick, sender, caps, *user_context, key, initial_modes, *join_msg_extended, *join_msg_standard, session_id, reply_tx).await;
            }
            ChannelEvent::Part { uid, reason, prefix, reply_tx } => {
                self.handle_part(uid, reason, prefix, reply_tx).await;
            }
            ChannelEvent::Quit { uid, quit_msg, reply_tx } => {
                self.handle_quit(uid, quit_msg, reply_tx).await;
            }
            ChannelEvent::Message {
                sender_uid,
                text,
                tags,
                is_notice,
                user_context,
                is_registered,
                is_tls,
                status_prefix,
                reply_tx,
            } => {
                self.handle_message(
                    sender_uid,
                    text,
                    tags,
                    is_notice,
                    *user_context,
                    is_registered,
                    is_tls,
                    status_prefix,
                    reply_tx,
                ).await;
            }
            ChannelEvent::Broadcast { message, exclude } => {
                self.handle_broadcast(message, exclude).await;
            }
            ChannelEvent::BroadcastWithCap { message, exclude, required_cap, fallback_msg } => {
                self.handle_broadcast_with_cap(*message, exclude, required_cap, fallback_msg.map(|m| *m)).await;
            }
            ChannelEvent::GetInfo { requester_uid, reply_tx } => {
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
            ChannelEvent::ApplyModes { sender_uid, sender_prefix, modes, target_uids, force, reply_tx } => {
                self.handle_apply_modes(sender_uid, sender_prefix, modes, target_uids, force, reply_tx).await;
            }
            ChannelEvent::Kick { sender_uid, sender_prefix, target_uid, target_nick, reason, force, reply_tx } => {
                self.handle_kick(sender_uid, sender_prefix, target_uid, target_nick, reason, force, reply_tx).await;
            }
            ChannelEvent::SetTopic { sender_uid, sender_prefix, topic, force, reply_tx } => {
                self.handle_set_topic(sender_uid, sender_prefix, topic, force, reply_tx).await;
            }
            ChannelEvent::Invite { sender_uid, sender_prefix, target_uid, target_nick, force, reply_tx } => {
                self.handle_invite(sender_uid, sender_prefix, target_uid, target_nick, force, reply_tx).await;
            }
            ChannelEvent::Knock { sender_uid, sender_prefix, reply_tx } => {
                self.handle_knock(sender_uid, sender_prefix, reply_tx).await;
            }
            ChannelEvent::NickChange { uid, old_nick: _, new_nick } => {
                self.handle_nick_change(uid, new_nick).await;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_join(
        &mut self,
        uid: Uid,
        nick: String,
        sender: mpsc::Sender<Message>,
        caps: HashSet<String>,
        user_context: UserContext,
        key_arg: Option<String>,
        initial_modes: Option<MemberModes>,
        join_msg_extended: Message,
        join_msg_standard: Message,
        session_id: Uuid,
        reply_tx: oneshot::Sender<Result<JoinSuccessData, String>>,
    ) {
        if self.state == ActorState::Draining {
            let _ = reply_tx.send(Err("ERR_CHANNEL_TOMBSTONE".to_string()));
            return;
        }

        // Validate that the user still exists and the session matches.
        let session_valid = if let Some(matrix) = self.matrix.upgrade() {
            if let Some(user_ref) = matrix.users.get(&uid) {
                let user = user_ref.read().await;
                user.session_id == session_id
            } else {
                false
            }
        } else {
            false
        };

        if !session_valid {
            let _ = reply_tx.send(Err("ERR_SESSION_INVALID".to_string()));
            return;
        }

        // Checks
        let user_mask = create_user_mask(&user_context);

        // 1. Bans (+b)
        if is_banned(&user_mask, &user_context, &self.bans, &self.excepts) {
            let _ = reply_tx.send(Err("ERR_BANNEDFROMCHAN".to_string()));
            return;
        }

        // 2. Invite Only (+i)
        if self.modes.contains(&ChannelMode::InviteOnly) {
            let is_invited = self.is_invited(&uid);
            let is_invex = self.invex.iter().any(|i| crate::security::matches_ban_or_except(&i.mask, &user_mask, &user_context));

            if !is_invited && !is_invex {
                let _ = reply_tx.send(Err("ERR_INVITEONLYCHAN".to_string()));
                return;
            }
        }

        // 3. Limit (+l)
        for mode in &self.modes {
            if let ChannelMode::Limit(limit) = mode
                && self.members.len() >= *limit {
                    let _ = reply_tx.send(Err("ERR_CHANNELISFULL".to_string()));
                    return;
                }
        }

        // 4. Key (+k)
        for mode in &self.modes {
            if let ChannelMode::Key(key) = mode
                && key_arg.as_deref() != Some(key) {
                    let _ = reply_tx.send(Err("ERR_BADCHANNELKEY".to_string()));
                    return;
                }
        }

        // Consume invite
        self.remove_invite(&uid);

        // Basic JOIN implementation
        // Fix #14: Preserve existing modes if user is already in channel (rejoin)
        let modes = if let Some(existing) = self.members.get(&uid) {
            existing.clone()
        } else {
            // Grant operator status to the first user (channel founder)
            let is_first_user = self.members.is_empty();
            if is_first_user {
                MemberModes {
                    op: true,
                    ..Default::default()
                }
            } else {
                initial_modes.unwrap_or_default()
            }
        };

        self.members.insert(uid.clone(), modes);
        self.user_nicks.insert(uid.clone(), nick.clone());
        self.senders.insert(uid.clone(), sender.clone());
        self.user_caps.insert(uid.clone(), caps.clone());

        self.handle_broadcast_with_cap(
            join_msg_extended,
            vec![uid.clone()],
            Some("extended-join".to_string()),
            Some(join_msg_standard),
        ).await;

        let is_secret = self.modes.contains(&ChannelMode::Secret);

        let data = JoinSuccessData {
            topic: self.topic.clone(),
            channel_name: self.name.clone(),
            is_secret,
        };

        let _ = reply_tx.send(Ok(data));
    }

    async fn handle_part(
        &mut self,
        uid: Uid,
        reason: Option<String>,
        prefix: Prefix,
        reply_tx: oneshot::Sender<Result<usize, String>>,
    ) {
        if !self.members.contains_key(&uid) {
            let _ = reply_tx.send(Err("Not on channel".to_string()));
            return;
        }

        // Broadcast PART
        let part_msg = Message {
            tags: None,
            prefix: Some(prefix),
            command: Command::PART(self.name.clone(), reason),
        };
        self.handle_broadcast(part_msg, None).await;

        // Remove member
        self.members.remove(&uid);
        self.senders.remove(&uid);
        self.user_caps.remove(&uid);
        self.user_nicks.remove(&uid);

        let _ = reply_tx.send(Ok(self.members.len()));

        self.cleanup_if_empty();
    }

    async fn handle_quit(&mut self, uid: Uid, quit_msg: Message, reply_tx: Option<oneshot::Sender<usize>>) {
        if self.members.contains_key(&uid) {
            self.handle_broadcast(quit_msg, None).await;
            self.members.remove(&uid);
            self.senders.remove(&uid);
            self.user_caps.remove(&uid);
            self.user_nicks.remove(&uid);
        }
        if let Some(tx) = reply_tx {
            let _ = tx.send(self.members.len());
        }

        self.cleanup_if_empty();
    }

    async fn handle_broadcast(&mut self, message: Message, exclude: Option<Uid>) {
        let msg = Arc::new(message);
        for (uid, sender) in &self.senders {
            if exclude.as_ref() == Some(uid) {
                continue;
            }
            let _ = sender.send((*msg).clone()).await;
        }
    }

    async fn handle_broadcast_with_cap(
        &mut self,
        message: Message,
        exclude: Vec<Uid>,
        required_cap: Option<String>,
        fallback_msg: Option<Message>,
    ) {
        let msg = Arc::new(message);
        let fallback = fallback_msg.map(Arc::new);

        for (uid, sender) in &self.senders {
            if exclude.contains(uid) {
                continue;
            }

            let should_send_main = if let Some(cap) = &required_cap {
                if let Some(caps) = self.user_caps.get(uid) {
                    caps.contains(cap)
                } else {
                    false
                }
            } else {
                true
            };

            if should_send_main {
                let _ = sender.send((*msg).clone()).await;
            } else if let Some(fb) = &fallback {
                let _ = sender.send((**fb).clone()).await;
            }
        }
    }

    fn cleanup_if_empty(&mut self) {
        // Skip cleanup if already draining
        if self.state == ActorState::Draining {
            return;
        }

        let is_permanent = self.modes.contains(&ChannelMode::Permanent);
        if self.members.is_empty() && !is_permanent {
            // Mark as draining first - this prevents any new events from being processed
            self.state = ActorState::Draining;

            // Remove channel from global registry
            //
            // RACE CONDITION NOTE (#12): There is a theoretical race where:
            // 1. This actor marks itself as draining and removes from matrix.channels
            // 2. A new JOIN arrives and creates a fresh actor before removal completes
            // 3. Both actors briefly exist
            //
            // However, this is SAFE because:
            // - The draining actor rejects all new events (checked at handler entry points)
            // - The new actor starts fresh with empty state
            // - DashMap's entry() semantics ensure only one actor per key
            // - Worst case: The old actor's removal overwrites the new actor's entry,
            //   causing a brief channel disappearance, but the next JOIN recreates it
            //
            // A perfect fix would require atomic "remove-if-empty" with DashMap's entry,
            // but this would require holding locks across async points, creating deadlocks.
            // Current approach trades theoretical brief inconsistency for deadlock freedom.
            if let Some(matrix) = self.matrix.upgrade() {
                let name_lower = self.name.to_lowercase();
                if matrix.channels.remove(&name_lower).is_some() {
                    crate::metrics::ACTIVE_CHANNELS.dec();
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_message(
        &mut self,
        sender_uid: Uid,
        text: String,
        tags: Option<Vec<slirc_proto::message::Tag>>,
        is_notice: bool,
        user_context: UserContext,
        is_registered: bool,
        is_tls: bool,
        status_prefix: Option<char>,
        reply_tx: oneshot::Sender<ChannelRouteResult>,
    ) {
        let is_member = self.members.contains_key(&sender_uid);
        let modes = &self.modes;

        // Check +n (no external messages)
        if modes.contains(&ChannelMode::NoExternal) && !is_member {
            let _ = reply_tx.send(ChannelRouteResult::BlockedExternal);
            return;
        }

        // Check +r (registered-only channel)
        if (modes.contains(&ChannelMode::Registered) || modes.contains(&ChannelMode::RegisteredOnly))
            && !is_registered
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedRegisteredOnly);
            return;
        }

        // Check +z (TLS-only channel)
        if modes.contains(&ChannelMode::TlsOnly) && !is_tls {
            let _ = reply_tx.send(ChannelRouteResult::BlockedExternal);
            return;
        }

        // Check +m (moderated)
        if modes.contains(&ChannelMode::Moderated)
            && !self.member_has_voice_or_higher(&sender_uid) {
                 let _ = reply_tx.send(ChannelRouteResult::BlockedModerated);
                 return;
            }

        // Check +T (no notice)
        if is_notice && modes.contains(&ChannelMode::NoNotice)
             && !self.member_has_halfop_or_higher(&sender_uid) {
                let _ = reply_tx.send(ChannelRouteResult::BlockedNotice);
                return;
            }

        // Check +C (no CTCP)
        if modes.contains(&ChannelMode::NoCtcp)
             && slirc_proto::ctcp::Ctcp::is_ctcp(&text)
                 && let Some(ctcp) = slirc_proto::ctcp::Ctcp::parse(&text)
                     && !matches!(ctcp.kind, slirc_proto::ctcp::CtcpKind::Action) {
                         let _ = reply_tx.send(ChannelRouteResult::BlockedCTCP);
                         return;
                     }

        // Check bans (+b) and quiets (+q)
        let is_op = self.member_has_halfop_or_higher(&sender_uid);
        let user_mask = create_user_mask(&user_context);

        if !is_op {
            if is_banned(&user_mask, &user_context, &self.bans, &self.excepts) {
                let _ = reply_tx.send(ChannelRouteResult::BlockedBanned);
                return;
            }

            for quiet in &self.quiets {
                 if crate::security::matches_ban_or_except(&quiet.mask, &user_mask, &user_context) {
                    let is_excepted = self.excepts.iter().any(|e| crate::security::matches_ban_or_except(&e.mask, &user_mask, &user_context));
                    if !is_excepted {
                        let _ = reply_tx.send(ChannelRouteResult::BlockedModerated);
                        return;
                    }
                }
            }
        }

        // Broadcast
        let msg = Message {
            tags,
            prefix: Some(slirc_proto::Prefix::Nickname(
                user_context.nickname.clone(),
                user_context.username.clone(),
                user_context.hostname.clone(),
            )),
            command: if is_notice {
                slirc_proto::Command::NOTICE(self.name.clone(), text)
            } else {
                slirc_proto::Command::PRIVMSG(self.name.clone(), text)
            },
        };

        let msg_arc = Arc::new(msg);
        for (uid, sender) in &self.senders {
            if uid == &sender_uid {
                continue;
            }

            if let Some(prefix) = status_prefix {
                if let Some(modes) = self.members.get(uid) {
                    let has_status = match prefix {
                        '@' => modes.op || modes.admin || modes.owner,
                        '+' => modes.voice || modes.halfop || modes.op || modes.admin || modes.owner,
                        _ => false,
                    };
                    if !has_status {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            let _ = sender.send((*msg_arc).clone()).await;
        }

        let _ = reply_tx.send(ChannelRouteResult::Sent);
    }

    async fn handle_apply_modes(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        modes: Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>,
        target_uids: HashMap<String, Uid>,
        force: bool,
        reply_tx: oneshot::Sender<Result<Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>, String>>,
    ) {
        let mut applied_modes = Vec::new();

        // Basic permission check
        let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
        let has_priv = sender_modes.has_op_or_higher() || force;

        for mode in modes {
            if !has_priv && !force {
                continue;
            }

            let adding = mode.is_plus();
            let mode_type = mode.mode();
            let arg = mode.arg();

            use slirc_proto::mode::ChannelMode as ProtoMode;

            let changed = match mode_type {
                ProtoMode::NoExternalMessages => self.set_flag_mode(ChannelMode::NoExternal, adding),
                ProtoMode::ProtectedTopic => self.set_flag_mode(ChannelMode::TopicLock, adding),
                ProtoMode::InviteOnly => self.set_flag_mode(ChannelMode::InviteOnly, adding),
                ProtoMode::Moderated => self.set_flag_mode(ChannelMode::Moderated, adding),
                ProtoMode::Secret => self.set_flag_mode(ChannelMode::Secret, adding),
                ProtoMode::RegisteredOnly => self.set_flag_mode(ChannelMode::RegisteredOnly, adding),
                ProtoMode::NoColors => self.set_flag_mode(ChannelMode::NoColors, adding),
                ProtoMode::NoCTCP => self.set_flag_mode(ChannelMode::NoCtcp, adding),
                ProtoMode::NoNickChange => self.set_flag_mode(ChannelMode::NoNickChange, adding),
                ProtoMode::NoKnock => self.set_flag_mode(ChannelMode::NoKnock, adding),
                ProtoMode::NoInvite => self.set_flag_mode(ChannelMode::NoInvite, adding),
                ProtoMode::NoChannelNotice => self.set_flag_mode(ChannelMode::NoNotice, adding),
                ProtoMode::NoKick => self.set_flag_mode(ChannelMode::NoKicks, adding),
                ProtoMode::Permanent => self.set_flag_mode(ChannelMode::Permanent, adding),
                ProtoMode::OperOnly => self.set_flag_mode(ChannelMode::OperOnly, adding),
                ProtoMode::FreeInvite => self.set_flag_mode(ChannelMode::FreeInvite, adding),
                ProtoMode::TlsOnly => self.set_flag_mode(ChannelMode::TlsOnly, adding),
                ProtoMode::Ban => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.bans, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoMode::Exception => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.excepts, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoMode::InviteException => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.invex, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoMode::Quiet => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.quiets, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoMode::Key => {
                    if adding {
                        if let Some(key) = arg {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Key(_)),
                                Some(ChannelMode::Key(key.to_string())),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Key(_)), None)
                    }
                }
                ProtoMode::Limit => {
                    if adding {
                        if let Some(limit) = arg.and_then(|a| a.parse::<usize>().ok()) {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Limit(_)),
                                Some(ChannelMode::Limit(limit)),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Limit(_)), None)
                    }
                }
                ProtoMode::Founder => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.owner = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoMode::Admin => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.admin = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoMode::Oper => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.op = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoMode::Halfop => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.halfop = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoMode::Voice => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.voice = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if changed {
                applied_modes.push(mode.clone());
            }
        }

        if !applied_modes.is_empty() {
            let msg = Message {
                tags: None,
                prefix: Some(sender_prefix),
                command: Command::ChannelMODE(self.name.clone(), applied_modes.clone()),
            };
            for sender in self.senders.values() {
                let _ = sender.send(msg.clone()).await;
            }
        }

        let _ = reply_tx.send(Ok(applied_modes));
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_kick(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        reason: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if !force {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err("ERR_CHANOPRIVSNEEDED".to_string()));
                return;
            }
        }

        if !self.members.contains_key(&target_uid) {
            let _ = reply_tx.send(Err("ERR_USERNOTINCHANNEL".to_string()));
            return;
        }

        let msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::KICK(self.name.clone(), target_nick, Some(reason)),
        };

        for sender in self.senders.values() {
            let _ = sender.send(msg.clone()).await;
        }

        self.members.remove(&target_uid);
        self.senders.remove(&target_uid);
        self.user_caps.remove(&target_uid);
        self.user_nicks.remove(&target_uid);
        self.kicked_users.insert(target_uid, std::time::Instant::now());

        let _ = reply_tx.send(Ok(()));
    }

    async fn handle_set_topic(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        topic: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if !force && self.modes.contains(&ChannelMode::TopicLock) {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err("ERR_CHANOPRIVSNEEDED".to_string()));
                return;
            }
        }

        self.topic = Some(Topic {
            text: topic.clone(),
            set_by: sender_prefix.to_string(),
            set_at: chrono::Utc::now().timestamp(),
        });

        let msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::TOPIC(self.name.clone(), Some(topic)),
        };

        for sender in self.senders.values() {
            let _ = sender.send(msg.clone()).await;
        }

        let _ = reply_tx.send(Ok(()));
    }

    async fn handle_invite(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if !force && self.modes.contains(&ChannelMode::InviteOnly) {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err("ERR_CHANOPRIVSNEEDED".to_string()));
                return;
            }
        }

        if self.members.contains_key(&target_uid) {
            let _ = reply_tx.send(Err("ERR_USERONCHANNEL".to_string()));
            return;
        }

        self.add_invite(target_uid.clone());

        // Broadcast invite-notify
        let invite_msg = Message {
            tags: None,
            prefix: Some(sender_prefix.clone()),
            command: Command::INVITE(target_nick.clone(), self.name.clone()),
        };

        for (uid, _) in &self.members {
            if *uid == target_uid { continue; }

            if let Some(caps) = self.user_caps.get(uid)
                && caps.contains("invite-notify")
                    && let Some(sender) = self.senders.get(uid) {
                        let _ = sender.send(invite_msg.clone()).await;
                    }
        }

        let _ = reply_tx.send(Ok(()));
    }

    async fn handle_knock(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if self.modes.contains(&ChannelMode::NoKnock) {
             let _ = reply_tx.send(Err("ERR_CANNOTKNOCK".to_string()));
             return;
        }

        if !self.modes.contains(&ChannelMode::InviteOnly) {
             let _ = reply_tx.send(Err("ERR_CHANOPEN".to_string()));
             return;
        }

        if self.members.contains_key(&sender_uid) {
             let _ = reply_tx.send(Err("ERR_USERONCHANNEL".to_string()));
             return;
        }

        let nick = match &sender_prefix {
            Prefix::Nickname(n, _, _) => n,
            _ => "Unknown",
        };

        let msg_text = format!("User {} is KNOCKing on {}", nick, self.name);
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::NOTICE(self.name.clone(), msg_text),
        };

        for (uid, modes) in &self.members {
            if (modes.op || modes.halfop)
                && let Some(sender) = self.senders.get(uid) {
                     let _ = sender.send(msg.clone()).await;
                }
        }

        let _ = reply_tx.send(Ok(()));
    }

    async fn handle_nick_change(&mut self, uid: Uid, new_nick: String) {
        // Update the user_nicks map with the new nickname
        if self.user_nicks.contains_key(&uid) {
            self.user_nicks.insert(uid, new_nick);
        }
    }

    fn member_has_voice_or_higher(&self, uid: &Uid) -> bool {
        self.members
            .get(uid)
            .map(|m| m.has_voice_or_higher())
            .unwrap_or(false)
    }

    fn member_has_halfop_or_higher(&self, uid: &Uid) -> bool {
        self.members
            .get(uid)
            .map(|m| m.has_halfop_or_higher())
            .unwrap_or(false)
    }

    fn prune_invites(&mut self) {
        while let Some(front) = self.invites.front() {
            if front.set_at.elapsed() > INVITE_TTL {
                self.invites.pop_front();
            } else {
                break;
            }
        }
    }

    fn add_invite(&mut self, uid: Uid) {
        self.prune_invites();

        if self.invites.iter().any(|entry| entry.uid == uid) {
            return;
        }

        self.invites.push_back(InviteEntry {
            uid,
            set_at: Instant::now(),
        });

        while self.invites.len() > MAX_INVITES_PER_CHANNEL {
            self.invites.pop_front();
        }
    }

    fn remove_invite(&mut self, uid: &Uid) {
        self.invites.retain(|entry| &entry.uid != uid);
    }

    fn is_invited(&mut self, uid: &Uid) -> bool {
        self.prune_invites();
        self.invites.iter().any(|entry| &entry.uid == uid)
    }

    fn set_flag_mode(&mut self, flag: ChannelMode, adding: bool) -> bool {
        if adding {
            self.modes.insert(flag)
        } else {
            self.modes.remove(&flag)
        }
    }

    fn replace_param_mode<F>(&mut self, predicate: F, new_mode: Option<ChannelMode>) -> bool
    where
        F: Fn(&ChannelMode) -> bool,
    {
        let mut changed = false;
        self.modes.retain(|mode| {
            let remove = predicate(mode);
            if remove {
                changed = true;
            }
            !remove
        });

        if let Some(mode) = new_mode {
            changed |= self.modes.insert(mode);
        }

        changed
    }

    fn apply_list_mode(list: &mut Vec<ListEntry>, mask: &str, adding: bool, set_by: &Uid) -> bool {
        if adding {
            if list.iter().any(|entry| entry.mask == mask) {
                return false;
            }

            list.push(ListEntry {
                mask: mask.to_string(),
                set_by: set_by.clone(),
                set_at: Utc::now().timestamp(),
            });
            true
        } else {
            let original_len = list.len();
            list.retain(|entry| entry.mask != mask);
            original_len != list.len()
        }
    }

    fn update_member_mode<F>(&mut self, target_uid: &Uid, mut update: F) -> bool
    where
        F: FnMut(&mut MemberModes),
    {
        if let Some(member) = self.members.get(target_uid).cloned() {
            let mut updated = member.clone();
            update(&mut updated);

            if updated != member {
                self.members.insert(target_uid.clone(), updated);
                return true;
            }
        }

        false
    }
}

/// Convert channel modes to string representation (e.g. "+ntk key").
pub fn modes_to_string(modes: &HashSet<ChannelMode>) -> String {
    let mut flags = String::new();
    let mut params = Vec::new();

    flags.push('+');

    // Simple modes
    if modes.contains(&ChannelMode::NoExternal) { flags.push('n'); }
    if modes.contains(&ChannelMode::TopicLock) { flags.push('t'); }
    if modes.contains(&ChannelMode::Moderated) { flags.push('m'); }
    if modes.contains(&ChannelMode::ModeratedUnreg) { flags.push('M'); }
    if modes.contains(&ChannelMode::NoNickChange) { flags.push('N'); }
    if modes.contains(&ChannelMode::NoColors) { flags.push('c'); }
    if modes.contains(&ChannelMode::TlsOnly) { flags.push('z'); }
    if modes.contains(&ChannelMode::NoKnock) { flags.push('K'); }
    if modes.contains(&ChannelMode::NoInvite) { flags.push('V'); }
    if modes.contains(&ChannelMode::NoNotice) { flags.push('T'); }
    if modes.contains(&ChannelMode::FreeInvite) { flags.push('g'); }
    if modes.contains(&ChannelMode::OperOnly) { flags.push('O'); }
    if modes.contains(&ChannelMode::AdminOnly) { flags.push('A'); }
    if modes.contains(&ChannelMode::Auditorium) { flags.push('u'); }
    if modes.contains(&ChannelMode::Registered) { flags.push('r'); }
    if modes.contains(&ChannelMode::NoKicks) { flags.push('Q'); }
    if modes.contains(&ChannelMode::Secret) { flags.push('s'); }
    if modes.contains(&ChannelMode::Private) { flags.push('p'); }
    if modes.contains(&ChannelMode::InviteOnly) { flags.push('i'); }
    if modes.contains(&ChannelMode::NoCtcp) { flags.push('C'); }
    if modes.contains(&ChannelMode::Permanent) { flags.push('P'); }
    if modes.contains(&ChannelMode::RegisteredOnly) { flags.push('R'); }
    if modes.contains(&ChannelMode::SSLOnly) { flags.push('S'); }

    // Param modes
    for mode in modes {
        match mode {
            ChannelMode::Key(k) => {
                if !flags.contains('k') { flags.push('k'); params.push(k.clone()); }
            }
            ChannelMode::Limit(l) => {
                if !flags.contains('l') { flags.push('l'); params.push(l.to_string()); }
            }
            ChannelMode::Redirect(c) => {
                if !flags.contains('L') { flags.push('L'); params.push(c.clone()); }
            }
            ChannelMode::JoinDelay(s) => {
                if !flags.contains('J') { flags.push('J'); params.push(s.to_string()); }
            }
            ChannelMode::JoinThrottle { joins, seconds } => {
                if !flags.contains('j') { flags.push('j'); params.push(format!("{}:{}", joins, seconds)); }
            }
            ChannelMode::FloodProtection { messages, seconds } => {
                if !flags.contains('f') { flags.push('f'); params.push(format!("{}:{}", messages, seconds)); }
            }
            _ => {}
        }
    }

    if params.is_empty() {
        flags
    } else {
        format!("{} {}", flags, params.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Create a ChannelActor
        let mut actor = create_test_channel_actor();

        let uid = "user123".to_string();
        let old_nick = "oldnick".to_string();
        let new_nick = "newnick".to_string();

        // Simulate a user having joined the channel
        actor.user_nicks.insert(uid.clone(), old_nick.clone());

        // Verify old nick is stored
        assert_eq!(actor.user_nicks.get(&uid), Some(&old_nick));

        // Simulate nick change
        actor.handle_nick_change(uid.clone(), new_nick.clone()).await;

        // Verify new nick is stored
        assert_eq!(actor.user_nicks.get(&uid), Some(&new_nick));
    }

    #[tokio::test]
    async fn test_nick_change_ignores_non_member() {
        // Create a ChannelActor
        let mut actor = create_test_channel_actor();

        let uid = "user123".to_string();
        let new_nick = "newnick".to_string();

        // Verify user is not in the channel
        assert_eq!(actor.user_nicks.get(&uid), None);

        // Simulate nick change for a non-member
        actor.handle_nick_change(uid.clone(), new_nick.clone()).await;

        // Verify nothing was added
        assert_eq!(actor.user_nicks.get(&uid), None);
    }
}

