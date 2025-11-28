//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.

use crate::config::{Config, OperBlock};
use crate::state::UidGenerator;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use slirc_proto::Message;

/// Unique user identifier (TS6 format: 9 characters).
pub type Uid = String;

/// Server identifier (TS6 format: 3 characters).
pub type Sid = String;

/// The Matrix - Central shared state container.
///
/// This is the core state of the IRC server, holding all users, channels,
/// and related data in thread-safe concurrent collections.
pub struct Matrix {
    /// All connected users, indexed by UID.
    pub users: DashMap<Uid, Arc<RwLock<User>>>,

    /// All channels, indexed by lowercase name.
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,

    /// Nick to UID mapping for fast nick lookups.
    pub nicks: DashMap<String, Uid>,

    /// UID to message sender mapping for routing.
    pub senders: DashMap<Uid, mpsc::Sender<Message>>,

    /// Connected servers (for future linking support).
    #[allow(dead_code)] // Phase 4+: Server linking
    pub servers: DashMap<Sid, Arc<Server>>,

    /// This server's identity.
    pub server_info: ServerInfo,

    /// UID generator for new connections.
    pub uid_gen: UidGenerator,

    /// Server configuration (for handlers to access).
    pub config: MatrixConfig,
}

/// Configuration accessible to handlers via Matrix.
#[derive(Debug, Clone)]
pub struct MatrixConfig {
    /// Server name for replies.
    pub server_name: String,
    /// Network name.
    #[allow(dead_code)] // Used in INFO replies
    pub network_name: String,
    /// Operator blocks.
    pub oper_blocks: Vec<OperBlock>,
}

/// This server's identity information.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read during welcome/info responses
pub struct ServerInfo {
    pub sid: Sid,
    pub name: String,
    pub network: String,
    pub description: String,
    pub created: i64,
}

/// A connected user.
#[derive(Debug)]
#[allow(dead_code)] // Fields used by WHOIS/WHO handlers
pub struct User {
    pub uid: Uid,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
    /// Channels this user is in (lowercase names).
    pub channels: HashSet<String>,
    /// User modes.
    pub modes: UserModes,
}

/// User modes.
#[derive(Debug, Default, Clone)]
pub struct UserModes {
    pub invisible: bool,      // +i
    pub wallops: bool,        // +w
    pub oper: bool,           // +o (IRC operator)
    pub registered: bool,     // +r (identified to NickServ)
    pub secure: bool,         // +Z (TLS connection)
}

impl UserModes {
    /// Convert modes to a string like "+iw".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        if self.invisible { s.push('i'); }
        if self.wallops { s.push('w'); }
        if self.oper { s.push('o'); }
        if self.registered { s.push('r'); }
        if self.secure { s.push('Z'); }
        if s == "+" {
            "+".to_string()
        } else {
            s
        }
    }
}

impl User {
    /// Create a new user.
    pub fn new(uid: Uid, nick: String, user: String, realname: String, host: String) -> Self {
        Self {
            uid,
            nick,
            user,
            realname,
            host,
            channels: HashSet::new(),
            modes: UserModes::default(),
        }
    }

    /// Get the user's prefix string (nick!user@host).
    #[allow(dead_code)] // Used in message prefix generation
    pub fn prefix(&self) -> String {
        format!("{}!{}@{}", self.nick, self.user, self.host)
    }
}

/// An IRC channel.
#[derive(Debug)]
pub struct Channel {
    pub name: String,
    pub topic: Option<Topic>,
    pub created: i64,
    /// Members: UID -> MemberModes
    pub members: HashMap<Uid, MemberModes>,
    /// Channel modes.
    pub modes: ChannelModes,
    /// Ban list (+b).
    pub bans: Vec<ListEntry>,
    /// Ban exception list (+e).
    pub excepts: Vec<ListEntry>,
    /// Invite exception list (+I).
    pub invex: Vec<ListEntry>,
    /// Quiet list (+q).
    pub quiets: Vec<ListEntry>,
}

/// Channel modes.
#[derive(Debug, Default, Clone)]
pub struct ChannelModes {
    pub invite_only: bool,      // +i
    pub moderated: bool,        // +m
    pub no_external: bool,      // +n
    pub secret: bool,           // +s
    pub topic_lock: bool,       // +t
    pub registered_only: bool,  // +r
    pub key: Option<String>,    // +k
    pub limit: Option<u32>,     // +l
}

impl ChannelModes {
    /// Convert modes to a string like "+nt".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        if self.invite_only { s.push('i'); }
        if self.moderated { s.push('m'); }
        if self.no_external { s.push('n'); }
        if self.secret { s.push('s'); }
        if self.topic_lock { s.push('t'); }
        if self.registered_only { s.push('r'); }
        if self.key.is_some() { s.push('k'); }
        if self.limit.is_some() { s.push('l'); }
        if s == "+" {
            "+".to_string()
        } else {
            s
        }
    }
}

/// An entry in a list (bans, excepts, invex).
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub mask: String,
    pub set_by: String,
    pub set_at: i64,
}

/// Channel topic with metadata.
#[derive(Debug, Clone)]
pub struct Topic {
    pub text: String,
    pub set_by: String,
    pub set_at: i64,
}

/// Member modes (op, voice, etc.).
#[derive(Debug, Default, Clone)]
pub struct MemberModes {
    pub op: bool,      // +o
    pub voice: bool,   // +v
}

impl MemberModes {
    /// Get the highest prefix character for this member.
    pub fn prefix_char(&self) -> Option<char> {
        if self.op {
            Some('@')
        } else if self.voice {
            Some('+')
        } else {
            None
        }
    }
}

impl Channel {
    /// Create a new channel.
    pub fn new(name: String) -> Self {
        Self {
            name,
            topic: None,
            created: chrono::Utc::now().timestamp(),
            members: HashMap::new(),
            modes: ChannelModes::default(),
            bans: Vec::new(),
            excepts: Vec::new(),
            invex: Vec::new(),
            quiets: Vec::new(),
        }
    }

    /// Add a member to the channel.
    pub fn add_member(&mut self, uid: Uid, modes: MemberModes) {
        self.members.insert(uid, modes);
    }

    /// Remove a member from the channel.
    pub fn remove_member(&mut self, uid: &str) -> bool {
        self.members.remove(uid).is_some()
    }

    /// Check if user is a member.
    pub fn is_member(&self, uid: &str) -> bool {
        self.members.contains_key(uid)
    }

    /// Check if user has op.
    pub fn is_op(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.op)
    }

    /// Check if user has voice or higher.
    #[allow(dead_code)] // Used for +m moderated channels
    pub fn can_speak(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.op || m.voice)
    }

    /// Get list of member UIDs.
    #[allow(dead_code)] // Used for channel-wide operations
    pub fn member_uids(&self) -> Vec<Uid> {
        self.members.keys().cloned().collect()
    }
}

/// A linked server (for future use).
#[derive(Debug)]
#[allow(dead_code)] // Phase 4+: Server linking
pub struct Server {
    pub sid: Sid,
    pub name: String,
    pub description: String,
}

impl Matrix {
    /// Create a new Matrix with the given server configuration.
    pub fn new(config: &Config) -> Self {
        let now = chrono::Utc::now().timestamp();

        Self {
            users: DashMap::new(),
            channels: DashMap::new(),
            nicks: DashMap::new(),
            senders: DashMap::new(),
            servers: DashMap::new(),
            server_info: ServerInfo {
                sid: config.server.sid.clone(),
                name: config.server.name.clone(),
                network: config.server.network.clone(),
                description: config.server.description.clone(),
                created: now,
            },
            uid_gen: UidGenerator::new(config.server.sid.clone()),
            config: MatrixConfig {
                server_name: config.server.name.clone(),
                network_name: config.server.network.clone(),
                oper_blocks: config.oper.clone(),
            },
        }
    }

    /// Register a user's message sender for routing.
    pub fn register_sender(&self, uid: &str, sender: mpsc::Sender<Message>) {
        self.senders.insert(uid.to_string(), sender);
    }

    /// Unregister a user's message sender.
    pub fn unregister_sender(&self, uid: &str) {
        self.senders.remove(uid);
    }

    /// Send a message to a specific user by UID.
    #[allow(dead_code)] // Used for direct user messaging
    pub async fn send_to_user(&self, uid: &str, msg: Message) -> bool {
        if let Some(sender) = self.senders.get(uid) {
            sender.send(msg).await.is_ok()
        } else {
            false
        }
    }

    /// Broadcast a message to all members of a channel.
    /// Optionally exclude one UID (usually the sender).
    /// Note: `channel_name` should already be lowercased by the caller.
    pub async fn broadcast_to_channel(&self, channel_name: &str, msg: Message, exclude: Option<&str>) {
        if let Some(channel) = self.channels.get(channel_name) {
            let channel = channel.read().await;
            for uid in channel.members.keys() {
                if exclude.is_some_and(|e| e == uid.as_str()) {
                    continue;
                }
                if let Some(sender) = self.senders.get(uid) {
                    let _ = sender.send(msg.clone()).await;
                }
            }
        }
    }
}
