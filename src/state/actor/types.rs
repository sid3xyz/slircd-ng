//! Type definitions for the channel actor model.
//!
//! Contains [`ChannelEvent`] variants and related types used for
//! message passing to [`ChannelActor`](super::ChannelActor) instances.

use crate::caps::{Cap, InviteCap, KickCap, TopicCap};
use crate::security::UserContext;
use crate::state::{ListEntry, MemberModes, Topic};
use slirc_proto::{Message, Prefix};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

// Re-export ChannelError from central error module
pub use crate::error::ChannelError;

/// Unique identifier for a user (UID string).
pub type Uid = String;

#[derive(Debug)]
pub struct JoinSuccessData {
    pub topic: Option<Topic>,
    pub channel_name: String,
    pub is_secret: bool,
}

// =============================================================================
// Parameter structs for actor handlers (reduces argument counts)
// =============================================================================

/// Parameters for JOIN event handling.
#[derive(Debug)]
pub struct JoinParams {
    pub uid: Uid,
    pub nick: String,
    pub sender: mpsc::Sender<Arc<Message>>,
    pub caps: HashSet<String>,
    pub user_context: UserContext,
    pub key: Option<String>,
    pub initial_modes: Option<MemberModes>,
    pub join_msg_extended: Message,
    pub join_msg_standard: Message,
    pub session_id: Uuid,
}

/// Parameters for KICK event handling.
#[derive(Debug)]
pub struct KickParams {
    pub sender_uid: Uid,
    pub sender_prefix: Prefix,
    pub target_uid: Uid,
    pub target_nick: String,
    pub reason: String,
    pub force: bool,
    pub cap: Option<Cap<KickCap>>,
}

/// Parameters for TOPIC event handling.
#[derive(Debug)]
pub struct TopicParams {
    pub sender_uid: Uid,
    pub sender_prefix: Prefix,
    pub topic: String,
    pub msgid: String,
    pub timestamp: String,
    pub force: bool,
    pub cap: Option<Cap<TopicCap>>,
}

/// Parameters for INVITE event handling.
#[derive(Debug)]
pub struct InviteParams {
    pub sender_uid: Uid,
    pub sender_prefix: Prefix,
    pub target_uid: Uid,
    pub target_nick: String,
    pub force: bool,
    pub cap: Option<Cap<InviteCap>>,
}

/// Parameters for MODE event handling.
#[derive(Debug)]
pub struct ModeParams {
    pub sender_uid: Uid,
    pub sender_prefix: Prefix,
    pub modes: Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>,
    pub target_uids: HashMap<String, Uid>,
    pub force: bool,
}

/// Parameters for channel MESSAGE event handling.
#[derive(Debug)]
pub struct ChannelMessageParams {
    pub sender_uid: Uid,
    pub text: String,
    pub tags: Option<Vec<slirc_proto::message::Tag>>,
    pub is_notice: bool,
    pub is_tagmsg: bool,
    pub user_context: UserContext,
    pub is_registered: bool,
    pub is_tls: bool,
    pub is_bot: bool,
    pub status_prefix: Option<char>,
    pub timestamp: Option<String>,
    pub msgid: Option<String>,
}

/// Events that can be sent to a Channel Actor.
#[derive(Debug)]
pub enum ChannelEvent {
    /// User joining the channel.
    Join {
        params: Box<JoinParams>,
        reply_tx: oneshot::Sender<Result<JoinSuccessData, ChannelError>>,
    },
    /// User leaving the channel.
    Part {
        uid: Uid,
        reason: Option<String>,
        prefix: Prefix,
        reply_tx: oneshot::Sender<Result<usize, ChannelError>>,
    },
    /// User quitting the server.
    Quit {
        uid: Uid,
        quit_msg: Message,
        reply_tx: Option<oneshot::Sender<usize>>,
    },
    /// User sending a message (PRIVMSG, NOTICE, or TAGMSG) to the channel.
    Message {
        params: Box<ChannelMessageParams>,
        reply_tx: oneshot::Sender<ChannelRouteResult>,
    },
    /// Request channel information (for LIST/WHO/NAMES).
    GetInfo {
        requester_uid: Option<Uid>,
        reply_tx: oneshot::Sender<ChannelInfo>,
    },
    /// Merge a CRDT representation into the channel (Innovation 2).
    MergeCrdt {
        crdt: Box<slirc_crdt::channel::ChannelCrdt>,
        source: Option<slirc_crdt::clock::ServerId>,
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
        params: ModeParams,
        reply_tx: oneshot::Sender<
            Result<Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>, ChannelError>,
        >,
    },
    /// Kick a user from the channel.
    Kick {
        params: KickParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    },
    /// Set the channel topic.
    SetTopic {
        params: TopicParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    },
    /// Invite a user to the channel.
    Invite {
        params: InviteParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    },
    /// Knock on the channel.
    Knock {
        sender_uid: Uid,
        sender_prefix: Prefix,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
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
    /// Update cached IRCv3 capabilities for a channel member.
    ///
    /// Channel actors keep a cached `user_caps` map for fast capability-gated broadcasts.
    /// This event keeps that cache in sync when a registered client performs mid-session
    /// `CAP REQ` changes.
    UpdateCaps { uid: Uid, caps: HashSet<String> },
    /// User nickname change.
    NickChange { uid: Uid, new_nick: String },
    /// Clear channel state (modes, bans, etc).
    Clear {
        sender_uid: Uid,
        sender_prefix: Prefix,
        target: ClearTarget,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    },
    /// Incoming TMODE from a peer server.
    RemoteMode {
        ts: u64,
        setter: String,
        modes: String,
        args: Vec<String>,
    },
    /// Incoming TOPIC from a peer server.
    RemoteTopic {
        ts: u64,
        setter: String,
        topic: String,
    },
    /// Incoming KICK from a peer server.
    RemoteKick {
        sender: String,
        target: String,
        reason: String,
    },
    /// Netsplit quit - remove user without broadcast (already handled by split.rs).
    NetsplitQuit {
        uid: Uid,
        reply_tx: oneshot::Sender<()>,
    },
}

/// Target for CLEAR command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearTarget {
    Modes,
    Bans,
    Ops,
    Voices,
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
    pub members: HashSet<Uid>,
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
    /// Sender is blocked by +M (registered-only speak).
    BlockedRegisteredSpeak,
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
    /// +U: Op Moderated (messages from non-ops only go to ops)
    OpModerated,
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
    /// +k <key>: Channel key required to join
    Key(String, slirc_crdt::clock::HybridTimestamp),
    /// +l <limit>: User limit
    Limit(usize, slirc_crdt::clock::HybridTimestamp),
}

#[derive(Debug, Clone)]
pub struct InviteEntry {
    pub uid: Uid,
    pub set_at: Instant,
}
