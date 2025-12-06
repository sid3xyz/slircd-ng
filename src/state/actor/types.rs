use crate::security::UserContext;
use crate::state::{ListEntry, MemberModes, Topic};
use slirc_proto::{Message, Prefix};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

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
        reply_tx: oneshot::Sender<
            Result<Vec<slirc_proto::mode::Mode<slirc_proto::mode::ChannelMode>>, String>,
        >,
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

#[derive(Debug, Clone)]
pub struct InviteEntry {
    pub uid: Uid,
    pub set_at: Instant,
}
