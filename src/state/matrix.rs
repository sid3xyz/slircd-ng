//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.

use crate::config::{Config, LimitsConfig, OperBlock, SecurityConfig};
use crate::security::{RateLimitManager, XLine};
use crate::state::UidGenerator;
use dashmap::{DashMap, DashSet};
use slirc_proto::Message;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Unique user identifier (TS6 format: 9 characters).
pub type Uid = String;

/// Server identifier (TS6 format: 3 characters).
pub type Sid = String;
use std::collections::VecDeque;

/// Maximum number of WHOWAS entries to keep per nickname.
const MAX_WHOWAS_PER_NICK: usize = 10;

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

    /// Nick enforcement timers: UID -> deadline when they will be renamed.
    pub enforce_timers: DashMap<Uid, Instant>,

    /// WHOWAS history: lowercase nick -> entries (most recent first).
    pub whowas: DashMap<String, VecDeque<WhowasEntry>>,

    /// Connected servers (for future linking support).
    #[allow(dead_code)] // Phase 4+: Server linking
    pub servers: DashMap<Sid, Arc<Server>>,

    /// This server's identity.
    pub server_info: ServerInfo,

    /// UID generator for new connections.
    pub uid_gen: UidGenerator,

    /// Server configuration (for handlers to access).
    pub config: MatrixConfig,

    /// Global rate limiter for flood protection.
    pub rate_limiter: RateLimitManager,

    /// Spam detection service for content analysis.
    pub spam_detector: Option<Arc<crate::security::spam::SpamDetectionService>>,

    /// Active X-lines (K/G/Z/R/S-lines) for server-level bans.
    /// Key is the pattern/mask, value is the XLine.
    pub xlines: DashMap<String, XLine>,

    /// Set of registered channel names (lowercase) for fast lookup.
    pub registered_channels: DashSet<String>,
}

/// An entry in the WHOWAS history for a disconnected user.
#[derive(Debug, Clone)]
pub struct WhowasEntry {
    /// The user's nickname (case-preserved).
    pub nick: String,
    /// The user's username.
    pub user: String,
    /// The user's hostname.
    pub host: String,
    /// The user's realname.
    pub realname: String,
    /// Server name they were connected to.
    pub server: String,
    /// When they logged out (Unix timestamp).
    pub logout_time: i64,
}

/// Configuration accessible to handlers via Matrix.
#[derive(Debug, Clone)]
pub struct MatrixConfig {
    /// Operator blocks.
    pub oper_blocks: Vec<OperBlock>,
    /// Rate limits (legacy - being replaced by security.rate_limits).
    #[allow(dead_code)]
    pub limits: LimitsConfig,
    /// Security configuration (cloaking, rate limiting).
    pub security: SecurityConfig,
}

/// This server's identity information.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    #[allow(dead_code)] // Phase 4+: Used in server-to-server linking
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
    /// Visible hostname shown to other users (cloaked for privacy).
    pub visible_host: String,
    /// Channels this user is in (lowercase names).
    pub channels: HashSet<String>,
    /// User modes.
    pub modes: UserModes,
    /// Account name if identified to NickServ.
    pub account: Option<String>,
    /// Away message if user is marked away (RFC 2812).
    pub away: Option<String>,
}

/// User modes.
#[derive(Debug, Default, Clone)]
pub struct UserModes {
    pub invisible: bool,  // +i
    pub wallops: bool,    // +w
    pub oper: bool,       // +o (IRC operator)
    pub registered: bool, // +r (identified to NickServ)
    pub secure: bool,     // +Z (TLS connection)
}

impl UserModes {
    /// Convert modes to a string like "+iw".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        if self.invisible {
            s.push('i');
        }
        if self.wallops {
            s.push('w');
        }
        if self.oper {
            s.push('o');
        }
        if self.registered {
            s.push('r');
        }
        if self.secure {
            s.push('Z');
        }
        if s == "+" { "+".to_string() } else { s }
    }
}

impl User {
    /// Create a new user.
    ///
    /// The `host` is cloaked using HMAC-SHA256 with the provided secret.
    pub fn new(
        uid: Uid,
        nick: String,
        user: String,
        realname: String,
        host: String,
        cloak_secret: &str,
        cloak_suffix: &str,
    ) -> Self {
        // Try to parse as IP for proper cloaking, fall back to hostname cloaking
        let visible_host = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            crate::security::cloaking::cloak_ip_hmac_with_suffix(&ip, cloak_secret, cloak_suffix)
        } else {
            crate::security::cloaking::cloak_hostname(&host, cloak_secret)
        };
        Self {
            uid,
            nick,
            user,
            realname,
            host,
            visible_host,
            channels: HashSet::new(),
            modes: UserModes::default(),
            account: None,
            away: None,
        }
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
    /// Extended ban list (bans with $ prefix like $a:account).
    pub extended_bans: Vec<ListEntry>,
}

/// Channel modes.
#[derive(Debug, Default, Clone)]
pub struct ChannelModes {
    pub invite_only: bool,     // +i
    pub moderated: bool,       // +m
    pub no_external: bool,     // +n
    pub secret: bool,          // +s
    pub topic_lock: bool,      // +t
    pub registered_only: bool, // +r
    pub key: Option<String>,   // +k
    pub limit: Option<u32>,    // +l
}

impl ChannelModes {
    /// Convert modes to a string like "+nt".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        if self.invite_only {
            s.push('i');
        }
        if self.moderated {
            s.push('m');
        }
        if self.no_external {
            s.push('n');
        }
        if self.secret {
            s.push('s');
        }
        if self.topic_lock {
            s.push('t');
        }
        if self.registered_only {
            s.push('r');
        }
        if self.key.is_some() {
            s.push('k');
        }
        if self.limit.is_some() {
            s.push('l');
        }
        if s == "+" { "+".to_string() } else { s }
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
    pub op: bool,    // +o
    pub voice: bool, // +v
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
            extended_bans: Vec::new(),
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
    #[allow(dead_code)] // TODO: Use for +m moderated channel enforcement
    pub fn can_speak(&self, uid: &str) -> bool {
        self.members.get(uid).is_some_and(|m| m.op || m.voice)
    }

    /// Get list of member UIDs.
    #[allow(dead_code)] // TODO: Use for WHO #channel and NAMES
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
    ///
    /// `registered_channels` is a list of channel names that are registered with ChanServ.
    /// These are stored in lowercase for fast lookup.
    pub fn new(config: &Config, registered_channels: Vec<String>) -> Self {
        use slirc_proto::irc_to_lower;

        let now = chrono::Utc::now().timestamp();

        // Build the registered channels set (lowercase for consistent lookup)
        let registered_set = DashSet::new();
        for name in registered_channels {
            registered_set.insert(irc_to_lower(&name));
        }

        Self {
            users: DashMap::new(),
            channels: DashMap::new(),
            nicks: DashMap::new(),
            senders: DashMap::new(),
            enforce_timers: DashMap::new(),
            whowas: DashMap::new(),
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
                oper_blocks: config.oper.clone(),
                limits: config.limits.clone(),
                security: config.security.clone(),
            },
            rate_limiter: RateLimitManager::new(config.security.rate_limits.clone()),
            spam_detector: if config.security.spam_detection_enabled {
                Some(Arc::new(crate::security::spam::SpamDetectionService::new()))
            } else {
                None
            },
            xlines: DashMap::new(),
            registered_channels: registered_set,
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
    #[allow(dead_code)] // TODO: Use for direct messaging (e.g., SQUERY replies)
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
    ///
    /// Uses `Arc<Message>` for efficient broadcasting to multiple recipients.
    pub async fn broadcast_to_channel(
        &self,
        channel_name: &str,
        msg: Message,
        exclude: Option<&str>,
    ) {
        if let Some(channel) = self.channels.get(channel_name) {
            let channel = channel.read().await;
            // Use Arc for efficient multi-recipient broadcasting
            let msg = Arc::new(msg);
            for uid in channel.members.keys() {
                if exclude.is_some_and(|e| e == uid.as_str()) {
                    continue;
                }
                if let Some(sender) = self.senders.get(uid) {
                    // Arc clone is just pointer copy (8 bytes)
                    let _ = sender.send((*msg).clone()).await;
                }
            }
        }
    }

    /// Disconnect a user from the server.
    ///
    /// This is the canonical kill logic, used by KILL, GHOST, and enforcement.
    /// It:
    /// 1. Records WHOWAS entry for historical queries
    /// 2. Removes user from all channels and broadcasts QUIT
    /// 3. Removes from nicks mapping
    /// 4. Removes from users collection
    /// 5. Drops the sender (terminates connection task)
    ///
    /// Returns the list of channels the user was in (for logging).
    pub async fn disconnect_user(&self, target_uid: &str, quit_reason: &str) -> Vec<String> {
        use slirc_proto::{Command, Prefix};

        // Get user info before removal
        let (nick, user, host, realname, user_channels) = {
            if let Some(user_ref) = self.users.get(target_uid) {
                let user = user_ref.read().await;
                (
                    user.nick.clone(),
                    user.user.clone(),
                    user.host.clone(),
                    user.realname.clone(),
                    user.channels.iter().cloned().collect::<Vec<_>>(),
                )
            } else {
                return vec![];
            }
        };

        // Record WHOWAS entry before user is removed
        self.record_whowas(&nick, &user, &host, &realname);

        // Build QUIT message
        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(nick.clone(), user, host)),
            command: Command::QUIT(Some(quit_reason.to_string())),
        };

        // Remove from channels and broadcast QUIT
        for channel_name in &user_channels {
            if let Some(channel_ref) = self.channels.get(channel_name) {
                let mut channel = channel_ref.write().await;
                channel.members.remove(target_uid);

                // Broadcast QUIT to remaining members
                for member_uid in channel.members.keys() {
                    if let Some(sender) = self.senders.get(member_uid) {
                        let _ = sender.send(quit_msg.clone()).await;
                    }
                }
            }
        }

        // Remove from nick mapping
        let nick_lower = slirc_proto::irc_to_lower(&nick);
        self.nicks.remove(&nick_lower);

        // Remove user from matrix
        self.users.remove(target_uid);

        // Remove enforcement timer if any
        self.enforce_timers.remove(target_uid);

        // Drop sender - this will cause the connection task to terminate
        self.senders.remove(target_uid);

        user_channels
    }

    /// Record a WHOWAS entry for a user who is disconnecting.
    ///
    /// Entries are stored per-nick (lowercase) with most recent first.
    /// Old entries are pruned to keep only MAX_WHOWAS_PER_NICK entries.
    pub fn record_whowas(&self, nick: &str, user: &str, host: &str, realname: &str) {
        let nick_lower = slirc_proto::irc_to_lower(nick);
        let entry = WhowasEntry {
            nick: nick.to_string(),
            user: user.to_string(),
            host: host.to_string(),
            realname: realname.to_string(),
            server: self.server_info.name.clone(),
            logout_time: chrono::Utc::now().timestamp(),
        };

        self.whowas.entry(nick_lower).or_default().push_front(entry);

        // Prune old entries if over the limit
        if let Some(mut entries) = self.whowas.get_mut(&slirc_proto::irc_to_lower(nick)) {
            while entries.len() > MAX_WHOWAS_PER_NICK {
                entries.pop_back();
            }
        }
    }

    /// Clean up expired WHOWAS entries.
    ///
    /// Removes entries older than 7 days. Call this periodically from a
    /// maintenance task to prevent unbounded growth.
    pub fn cleanup_whowas(&self, max_age_days: i64) {
        let cutoff = chrono::Utc::now().timestamp() - (max_age_days * 24 * 3600);

        self.whowas.retain(|_, entries| {
            entries.retain(|e| e.logout_time > cutoff);
            !entries.is_empty()
        });
    }
}
