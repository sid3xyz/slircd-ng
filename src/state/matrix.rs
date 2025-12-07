//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.
//!
//! # Lock Order (Deadlock Prevention)
//!
//! When acquiring multiple locks, always follow this order:
//!
//! 1. DashMap shard lock (acquired during `.get()` / `.iter()`)
//! 2. Channel `RwLock` (read or write)
//! 3. User `RwLock` (read or write)
//!
//! **Never acquire locks in reverse order.** For example, never hold a User
//! write lock and then try to access a Channel or iterate the DashMap.
//!
//! Safe patterns used throughout the codebase:
//! - **Read-only iteration**: Iterate DashMap, acquire read locks inside loop
//! - **Collect-then-mutate**: Collect UIDs/keys to Vec, release iteration, then mutate
//! - **Lock-copy-release**: Acquire lock, copy needed data, release before next operation

use super::user::{User, WhowasEntry};
use crate::db::Database;
use crate::services::{chanserv, nickserv};

use crate::config::{Config, LimitsConfig, OperBlock, SecurityConfig, ServerConfig};
use crate::db::{Dline, Gline, Kline, Shun, Zline};
use crate::security::{BanCache, IpDenyList, RateLimitManager};
use crate::state::UidGenerator;
use crate::state::actor::ChannelEvent;
use dashmap::{DashMap, DashSet};
use slirc_proto::Message;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use tokio::sync::{RwLock, broadcast, mpsc};

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
    pub channels: DashMap<String, mpsc::Sender<ChannelEvent>>,

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

    /// In-memory ban cache for fast connection-time ban checks.
    pub ban_cache: BanCache,

    /// High-performance IP deny list (Roaring Bitmap engine).
    /// Used for nanosecond-scale IP rejection in the gateway accept loop.
    pub ip_deny_list: std::sync::RwLock<IpDenyList>,

    /// NickServ service singleton.
    pub nickserv: nickserv::NickServ,

    /// ChanServ service singleton.
    pub chanserv: chanserv::ChanServ,

    /// Maximum local user count (historical peak).
    pub max_local_users: AtomicUsize,

    /// Maximum global user count (historical peak).
    pub max_global_users: AtomicUsize,

    /// Shutdown signal broadcaster.
    /// When DIE command is issued, a message is sent on this channel.
    pub shutdown_tx: broadcast::Sender<()>,
}

/// Configuration accessible to handlers via Matrix.
#[derive(Debug, Clone)]
pub struct MatrixConfig {
    /// Server configuration (name, network, password, etc.).
    pub server: ServerConfig,
    /// Operator blocks.
    pub oper_blocks: Vec<OperBlock>,
    /// Rate limits (legacy - being replaced by security.rate_limits).
    #[allow(dead_code)]
    pub limits: LimitsConfig,
    /// Security configuration (cloaking, rate limiting).
    pub security: SecurityConfig,
    /// Account registration configuration.
    pub account_registration: crate::config::AccountRegistrationConfig,
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
    /// MOTD lines loaded from config file.
    pub motd_lines: Vec<String>,
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
    /// # Arguments
    /// - `config`: Server configuration
    /// - `data_dir`: Directory for data files (IP deny list, etc.)
    /// - `db`: Database handle for services
    /// - `registered_channels`: Channel names registered with ChanServ (stored lowercase)
    /// - `shuns`: Active shuns loaded from database
    /// - `klines`: Active K-lines loaded from database
    /// - `dlines`: Active D-lines loaded from database (synced to IpDenyList)
    /// - `glines`: Active G-lines loaded from database
    /// - `zlines`: Active Z-lines loaded from database (synced to IpDenyList)
    #[allow(clippy::too_many_arguments)] // Startup initialization requires many data sources
    pub fn new(
        config: &Config,
        data_dir: Option<&std::path::Path>,
        db: Database,
        registered_channels: Vec<String>,
        shuns: Vec<Shun>,
        klines: Vec<Kline>,
        dlines: Vec<Dline>,
        glines: Vec<Gline>,
        zlines: Vec<Zline>,
    ) -> Self {
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

        // Load IP deny list from data directory
        let ip_deny_path = data_dir
            .map(|d| d.join("ip_bans.msgpack"))
            .unwrap_or_else(|| std::path::PathBuf::from("ip_bans.msgpack"));
        let mut ip_deny_list = IpDenyList::load(&ip_deny_path);

        // Sync IpDenyList with database D-lines and Z-lines
        // This ensures any bans added via database admin tools are in the bitmap
        ip_deny_list.sync_from_database_bans(&dlines, &zlines);

        // Build the ban cache (K-lines and G-lines only; IP bans handled by IpDenyList)
        let ban_cache = BanCache::load(klines, glines);

        // Create shutdown broadcast channel
        let (shutdown_tx, _) = broadcast::channel(1);

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
                motd_lines: config.motd.load_lines(),
            },
            uid_gen: UidGenerator::new(config.server.sid.clone()),
            config: MatrixConfig {
                server: config.server.clone(),
                oper_blocks: config.oper.clone(),
                limits: config.limits.clone(),
                security: config.security.clone(),
                account_registration: config.account_registration.clone(),
            },
            rate_limiter: RateLimitManager::new(config.security.rate_limits.clone()),
            spam_detector: if config.security.spam_detection_enabled {
                Some(Arc::new(crate::security::spam::SpamDetectionService::new()))
            } else {
                None
            },
            registered_channels: registered_set,
            shuns: shuns_map,
            monitors: DashMap::new(),
            monitoring: DashMap::new(),
            ban_cache,
            ip_deny_list: std::sync::RwLock::new(ip_deny_list),
            nickserv: nickserv::NickServ::new(db.clone()),
            chanserv: chanserv::ChanServ::new(db),
            max_local_users: AtomicUsize::new(0),
            max_global_users: AtomicUsize::new(0),
            shutdown_tx,
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
        if let Some(sender) = self.channels.get(channel_name) {
            let _ = sender
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
    ///
    /// Returns count of messages sent.
    pub async fn broadcast_to_channel_with_cap(
        &self,
        channel_name: &str,
        msg: Message,
        exclude: Option<&str>,
        required_cap: Option<&str>,
        fallback_msg: Option<Message>,
    ) -> usize {
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
    ) -> usize {
        if let Some(sender) = self.channels.get(channel_name) {
            let _ = sender
                .send(ChannelEvent::BroadcastWithCap {
                    message: Box::new(msg),
                    exclude: exclude.iter().map(|s| s.to_string()).collect(),
                    required_cap: required_cap.map(|s| s.to_string()),
                    fallback_msg: fallback_msg.map(Box::new),
                })
                .await;
        }
        0
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
            if let Some(sender) = self.channels.get(channel_name) {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = sender
                    .send(ChannelEvent::Quit {
                        uid: target_uid.to_string(),
                        quit_msg: quit_msg.clone(),
                        reply_tx: Some(tx),
                    })
                    .await;

                if let Ok(remaining) = rx.await
                    && remaining == 0
                {
                    self.channels.remove(channel_name);
                    crate::metrics::ACTIVE_CHANNELS.dec();
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

    /// Broadcast CAP NEW or CAP DEL to all connected clients with cap-notify enabled.
    ///
    /// This is called when the server dynamically adds or removes capabilities.
    /// Per IRCv3 cap-notify spec: <https://ircv3.net/specs/extensions/capability-negotiation#cap-notify>
    ///
    /// # Arguments
    /// * `is_new` - true for CAP NEW, false for CAP DEL
    /// * `caps` - list of capability names being added or removed
    #[allow(dead_code)] // Infrastructure for dynamic capability changes (e.g., SASL backend up/down)
    pub async fn broadcast_cap_change(&self, is_new: bool, caps: &[&str]) {
        use slirc_proto::{CapSubCommand, Command, Prefix};

        if caps.is_empty() {
            return;
        }

        let subcommand = if is_new {
            CapSubCommand::NEW
        } else {
            CapSubCommand::DEL
        };
        let caps_str = caps.join(" ");

        let msg = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(self.server_info.name.clone())),
            command: Command::CAP(Some("*".to_string()), subcommand, None, Some(caps_str)),
        };

        // Send to all users who have cap-notify enabled
        for user_ref in self.users.iter() {
            let user = user_ref.read().await;
            if user.caps.contains("cap-notify")
                && let Some(sender) = self.senders.get(&user.uid)
            {
                let _ = sender.send(msg.clone()).await;
            }
        }
    }
}
