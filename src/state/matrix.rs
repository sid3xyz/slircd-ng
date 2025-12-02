//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.

use super::channel::{Channel, ListEntry, MemberModes, Topic};
use super::user::{User, WhowasEntry};

use crate::config::{Config, LimitsConfig, OperBlock, SecurityConfig};
use crate::db::Shun;
use crate::security::{RateLimitManager, XLine};
use crate::state::UidGenerator;
use dashmap::{DashMap, DashSet};
use slirc_proto::Message;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Unique user identifier (TS6 format: 9 characters).
pub type Uid = String;

/// Server identifier (TS6 format: 3 characters).
pub type Sid = String;

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

    /// Active shuns cached in memory for fast lookup.
    /// Key is the mask pattern, value is the Shun record.
    pub shuns: DashMap<String, Shun>,

    /// MONITOR: Nicknames being monitored by each UID.
    /// Key is UID, value is set of lowercase nicknames.
    pub monitors: DashMap<Uid, DashSet<String>>,

    /// MONITOR: Reverse mapping - who is monitoring each nickname.
    /// Key is lowercase nickname, value is set of UIDs monitoring it.
    pub monitoring: DashMap<String, DashSet<Uid>>,
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
    /// `shuns` is a list of active shuns loaded from the database.
    pub fn new(config: &Config, registered_channels: Vec<String>, shuns: Vec<Shun>) -> Self {
        use slirc_proto::irc_to_lower;

        let now = chrono::Utc::now().timestamp();

        // Build the registered channels set (lowercase for consistent lookup)
        let registered_set = DashSet::new();
        for name in registered_channels {
            registered_set.insert(irc_to_lower(&name));
        }

        // Build the shuns map
        let shuns_map = DashMap::new();
        for shun in shuns {
            shuns_map.insert(shun.mask.clone(), shun);
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
            shuns: shuns_map,
            monitors: DashMap::new(),
            monitoring: DashMap::new(),
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
