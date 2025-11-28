//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.

use crate::config::ServerConfig;
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
    pub servers: DashMap<Sid, Arc<Server>>,

    /// This server's identity.
    pub server_info: ServerInfo,

    /// UID generator for new connections.
    pub uid_gen: UidGenerator,
}

/// This server's identity information.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub sid: Sid,
    pub name: String,
    pub network: String,
    pub description: String,
    pub created: i64,
}

/// A connected user.
#[derive(Debug)]
pub struct User {
    pub uid: Uid,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
    /// Channels this user is in (lowercase names).
    pub channels: HashSet<String>,
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
        }
    }

    /// Get the user's prefix string (nick!user@host).
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
        self.members.get(uid).map(|m| m.op).unwrap_or(false)
    }

    /// Check if user has voice or higher.
    pub fn can_speak(&self, uid: &str) -> bool {
        self.members.get(uid).map(|m| m.op || m.voice).unwrap_or(false)
    }

    /// Get list of member UIDs.
    pub fn member_uids(&self) -> Vec<Uid> {
        self.members.keys().cloned().collect()
    }
}

/// A linked server (for future use).
#[derive(Debug)]
pub struct Server {
    pub sid: Sid,
    pub name: String,
    pub description: String,
}

impl Matrix {
    /// Create a new Matrix with the given server configuration.
    pub fn new(config: &ServerConfig) -> Self {
        let now = chrono::Utc::now().timestamp();

        Self {
            users: DashMap::new(),
            channels: DashMap::new(),
            nicks: DashMap::new(),
            senders: DashMap::new(),
            servers: DashMap::new(),
            server_info: ServerInfo {
                sid: config.sid.clone(),
                name: config.name.clone(),
                network: config.network.clone(),
                description: config.description.clone(),
                created: now,
            },
            uid_gen: UidGenerator::new(config.sid.clone()),
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
    pub async fn send_to_user(&self, uid: &str, msg: Message) -> bool {
        if let Some(sender) = self.senders.get(uid) {
            sender.send(msg).await.is_ok()
        } else {
            false
        }
    }

    /// Broadcast a message to all members of a channel.
    /// Optionally exclude one UID (usually the sender).
    pub async fn broadcast_to_channel(&self, channel_name: &str, msg: Message, exclude: Option<&str>) {
        let channel_lower = slirc_proto::irc_to_lower(channel_name);
        
        if let Some(channel) = self.channels.get(&channel_lower) {
            let channel = channel.read().await;
            for uid in channel.members.keys() {
                if exclude.map(|e| e == uid).unwrap_or(false) {
                    continue;
                }
                if let Some(sender) = self.senders.get(uid) {
                    let _ = sender.send(msg.clone()).await;
                }
            }
        }
    }
}
