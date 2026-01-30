//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix acts as a central dependency injection container and coordinator
//! for the various domain managers that hold the actual server state.
//!
//! # Architecture
//!
//! Following a major refactor to eliminate "God Object" patterns, the Matrix
//! delegates state and behavior to specialized managers:
//! - [`UserManager`]: Handles users, nicknames, and WHOWAS history.
//! - [`ChannelManager`]: Handles channel actors and broadcasting.
//! - [`SecurityManager`]: Handles bans, rate limiting, and spam detection.
//! - [`ServiceManager`]: Handles internal services (NickServ, ChanServ) and history.
//! - [`MonitorManager`]: Handles IRCv3 MONITOR lists.
//! - [`LifecycleManager`]: Handles server shutdown and disconnect signaling.
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

use crate::db::Database;
use crate::state::client::SessionId;
use crate::state::managers::client::ClientManager;
use crate::state::{
    ChannelManager, LifecycleManager, MonitorManager, SecurityManager, SecurityManagerParams,
    ServiceManager, SyncManager, Uid, UserManager,
};
use parking_lot::RwLock;
use slirc_proto::sync::clock::ServerId;

use crate::config::{Config, OperBlock, SecurityConfig, ServerConfig};
use crate::handlers::{cleanup_monitors, notify_monitors_offline};
use crate::state::actor::ChannelEvent;
use slirc_proto::Message;
use std::sync::Arc;
use tokio::sync::mpsc;

/// The Matrix - Central shared state container.
///
/// This is the core state of the IRC server, holding all users, channels,
/// and related data in thread-safe concurrent collections.
pub struct Matrix {
    /// User management state.
    pub user_manager: UserManager,

    /// Channel management state.
    pub channel_manager: ChannelManager,

    /// Client management state (bouncer/multiclient support).
    pub client_manager: ClientManager,

    /// Security management state.
    pub security_manager: SecurityManager,

    /// Service management state.
    pub service_manager: ServiceManager,

    /// Monitor management state.
    pub monitor_manager: MonitorManager,

    /// Lifecycle management state.
    pub lifecycle_manager: LifecycleManager,

    /// Sync management state (Innovation 2: Distributed Server Linking).
    pub sync_manager: SyncManager,

    /// Runtime statistics (user/channel counts, uptime).
    pub stats_manager: Arc<crate::state::managers::stats::StatsManager>,

    /// Read marker management state (Unified Read State).
    pub read_marker_manager: Arc<crate::state::managers::read_markers::ReadMarkersManager>,

    /// This server's identity.
    pub server_info: ServerInfo,

    /// Server ID for CRDT synchronization.
    pub server_id: ServerId,

    /// Server configuration (for handlers to access).
    pub config: MatrixConfig,

    /// Path to the configuration file (for REHASH to reload from).
    pub config_path: String,

    /// Hot-reloadable configuration (REHASH safe).
    /// Use `hot_config.read()` to access, `hot_config.write()` to update atomically.
    pub hot_config: RwLock<HotConfig>,

    /// Router channel for remote messages.
    pub router_tx: mpsc::Sender<Arc<Message>>,

    /// Database handle for server-wide persistence.
    pub db: crate::db::Database,
}

/// Configuration accessible to handlers via Matrix.
#[derive(Debug, Clone)]
pub struct MatrixConfig {
    /// Server configuration (name, network, password, etc.).
    pub server: ServerConfig,
    /// Operator blocks.
    pub oper_blocks: Vec<OperBlock>,
    /// Security configuration (cloaking, rate limiting).
    pub security: SecurityConfig,
    /// Account registration configuration.
    pub account_registration: crate::config::AccountRegistrationConfig,
    /// Multiclient/bouncer configuration.
    pub multiclient: crate::config::MulticlientConfig,
    /// Command output limits (WHO, LIST, NAMES).
    pub limits: crate::config::LimitsConfig,
    /// History configuration (Innovation 5: Event-Sourced History).
    pub history: crate::config::HistoryConfig,
    /// Link blocks for server peering.
    pub links: Vec<crate::config::LinkBlock>,
    /// TLS configuration (for STS capability advertising).
    pub tls: Option<crate::config::TlsConfig>,
}

/// Hot-reloadable configuration fields that can be atomically swapped via REHASH.
/// Access via `Matrix::hot_config.read()` or `Matrix::hot_config.write()`.
#[derive(Debug, Clone)]
pub struct HotConfig {
    /// Server description (shown in RPL_INFO, LUSERS).
    pub description: String,
    /// MOTD lines (shown in RPL_MOTD).
    pub motd_lines: Vec<String>,
    /// Operator blocks (for oper authentication).
    pub oper_blocks: Vec<OperBlock>,
    /// Admin info lines (RPL_ADMINLOC1, RPL_ADMINLOC2, RPL_ADMINEMAIL).
    pub admin_info: (Option<String>, Option<String>, Option<String>),
}

impl HotConfig {
    /// Create a new HotConfig from a Config reference.
    pub fn from_config(config: &Config) -> Self {
        Self {
            description: config.server.description.clone(),
            motd_lines: config.motd.load_lines(),
            oper_blocks: config.oper.clone(),
            admin_info: (
                config.server.admin_info1.clone(),
                config.server.admin_info2.clone(),
                config.server.admin_email.clone(),
            ),
        }
    }
}

/// This server's identity information.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub network: String,
    pub sid: String,
    pub description: String,
    #[allow(dead_code)]
    pub created: i64,
    /// MOTD lines loaded from config file.
    pub motd_lines: Vec<String>,
    /// Idle timeout configuration for ping/pong keepalive.
    pub idle_timeouts: crate::config::IdleTimeoutsConfig,
}

/// Parameters for creating a new Matrix.
pub struct MatrixParams<'a> {
    pub config: &'a Config,
    /// Path to the configuration file (for REHASH).
    pub config_path: String,
    pub data_dir: Option<&'a std::path::Path>,
    pub db: Database,
    pub history: std::sync::Arc<dyn crate::history::HistoryProvider>,
    pub registered_channels: Vec<String>,
    pub shuns: Vec<crate::db::Shun>,
    pub klines: Vec<crate::db::Kline>,
    pub dlines: Vec<crate::db::Dline>,
    pub glines: Vec<crate::db::Gline>,
    pub zlines: Vec<crate::db::Zline>,
    pub disconnect_tx: mpsc::Sender<(Uid, String)>,
    /// Optional always-on store for bouncer persistence.
    pub always_on_store: Option<std::sync::Arc<crate::db::AlwaysOnStore>>,
}

/// Data required to perform a user disconnect.
struct UserDisconnectInfo {
    nick: String,
    user: String,
    host: String,
    realname: String,
    channels: Vec<String>,
    session_id: SessionId,
    account: Option<String>,
    is_invisible: bool,
    is_oper: bool,
}

impl Matrix {
    /// Create a new Matrix with the given server configuration.
    pub fn new(params: MatrixParams<'_>) -> (Self, mpsc::Receiver<Arc<Message>>) {
        let MatrixParams {
            config,
            config_path,
            data_dir,
            db,
            history,
            registered_channels,
            shuns,
            klines,
            dlines,
            glines,
            zlines,
            disconnect_tx,
            always_on_store,
        } = params;

        use slirc_proto::irc_to_lower;

        let now = chrono::Utc::now().timestamp();

        // Build the registered channels set (lowercase for consistent lookup)
        let registered_channel_names: Vec<String> = registered_channels
            .into_iter()
            .map(|name| irc_to_lower(&name))
            .collect();

        let server_id = ServerId::new(config.server.sid.clone());
        let sync_manager = SyncManager::new(
            server_id.clone(),
            config.server.name.clone(),
            config.server.description.clone(),
            config.links.clone(),
            &config.security.rate_limits,
        );
        let sync_manager_arc = Arc::new(sync_manager);
        let mut user_manager =
            UserManager::new(config.server.sid.clone(), config.server.name.clone());
        user_manager.set_observer(sync_manager_arc.clone());

        let stats_manager = Arc::new(crate::state::managers::stats::StatsManager::new());
        user_manager.set_stats_manager(stats_manager.clone());

        let mut channel_manager = ChannelManager::with_registered_channels(
            registered_channel_names,
            stats_manager.clone(),
        );
        channel_manager.set_observer(sync_manager_arc.clone());

        // Create ReadMarkersManager (Unified Read State)
        let read_marker_manager = Arc::new(
            crate::state::managers::read_markers::ReadMarkersManager::new(always_on_store.clone()),
        );

        // Create ServiceManager with server SID for service UIDs
        let service_manager = ServiceManager::new(db.clone(), history, &config.server.sid);

        // Register service pseudoclients in UserManager
        let service_users = service_manager.create_service_users(&config.server.name, &server_id);
        for user in service_users {
            user_manager.register_service_user(user);
        }

        let (router_tx, router_rx) = mpsc::channel(1000);

        // Create ClientManager with optional always-on store
        let client_manager = match always_on_store {
            Some(store) => {
                ClientManager::with_store(store, config.multiclient.max_sessions_per_account)
            }
            None => ClientManager::with_max_sessions(config.multiclient.max_sessions_per_account),
        };

        (
            Self {
                user_manager,
                channel_manager,
                client_manager,
                security_manager: SecurityManager::new(SecurityManagerParams {
                    security_config: &config.security,
                    db: Some(db.clone()),
                    data_dir,
                    shuns,
                    klines,
                    dlines,
                    glines,
                    zlines,
                }),
                service_manager,
                monitor_manager: MonitorManager::new(),
                lifecycle_manager: LifecycleManager::new(disconnect_tx),
                sync_manager: Arc::try_unwrap(sync_manager_arc)
                    .unwrap_or_else(|arc| (*arc).clone()),
                stats_manager,
                read_marker_manager,
                server_info: ServerInfo {
                    name: config.server.name.clone(),
                    network: config.server.network.clone(),
                    sid: config.server.sid.clone(),
                    description: config.server.description.clone(),
                    created: now,
                    motd_lines: config.motd.load_lines(),
                    idle_timeouts: config.server.idle_timeouts.clone(),
                },
                server_id,
                config: MatrixConfig {
                    server: config.server.clone(),
                    oper_blocks: config.oper.clone(),
                    security: config.security.clone(),
                    account_registration: config.account_registration.clone(),
                    multiclient: config.multiclient.clone(),
                    limits: config.limits.clone(),
                    history: config.history.clone(),
                    links: config.links.clone(),
                    tls: config.tls.clone(),
                },
                config_path,
                hot_config: RwLock::new(HotConfig::from_config(config)),
                router_tx,
                db,
            },
            router_rx,
        )
    }

    /// Register a session's message sender for routing, along with its capabilities.
    /// For bouncer mode, multiple sessions may share a UID, so we append to the list.
    pub fn register_session_sender(
        &self,
        uid: &str,
        session_id: crate::state::client::SessionId,
        sender: mpsc::Sender<Arc<Message>>,
        caps: std::collections::HashSet<String>,
    ) {
        self.user_manager
            .register_session_sender(uid, session_id, sender, caps);
    }

    /// Get the current hybrid timestamp for CRDT operations.
    pub fn clock(&self) -> slirc_proto::sync::clock::HybridTimestamp {
        slirc_proto::sync::clock::HybridTimestamp::now(&self.server_id)
    }

    /// Request that a user be disconnected.
    ///
    /// This is safe to call from channel actors because it is non-blocking.
    /// Uses try_send to avoid blocking when the channel is full - in that case
    /// the disconnect will be silently dropped (which is acceptable since the
    /// disconnect worker will catch up eventually).
    pub fn request_disconnect(&self, uid: &str, reason: &str) {
        self.lifecycle_manager.request_disconnect(uid, reason);
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
    pub async fn disconnect_user(
        self: &Arc<Self>,
        target_uid: &Uid,
        quit_reason: &str,
    ) -> Vec<String> {
        // When session_id is not provided, fall back to looking it up from User
        // This is used by KILL and other commands that don't have a session context
        self.disconnect_user_session(target_uid, quit_reason, None)
            .await
    }

    /// Disconnect a user session with explicit session_id.
    ///
    /// If session_id is provided, use it for session tracking.
    /// If None, fall back to looking it up from the User object.
    pub async fn disconnect_user_session(
        self: &Arc<Self>,
        target_uid: &Uid,
        quit_reason: &str,
        explicit_session_id: Option<SessionId>,
    ) -> Vec<String> {
        // 1. Fetch User Info
        let info = match self
            .fetch_user_disconnect_info(target_uid, explicit_session_id)
            .await
        {
            Some(i) => i,
            None => return vec![],
        };

        // 2. Handle Session Detachment (Bouncer/Multiclient)
        if self.process_session_detachment(target_uid, &info).await {
            return info.channels;
        }

        // 3. Cleanup Monitors & Record WHOWAS
        self.cleanup_monitors_and_whowas(target_uid, &info).await;

        // 4. Leave Channels & Broadcast QUIT
        self.broadcast_quit_and_leave_channels(target_uid, &info, quit_reason)
            .await;

        // 5. Final Cleanup (Maps, Timers, Metrics)
        self.cleanup_user_state(target_uid, &info).await;

        info.channels
    }

    // --- Helper Methods ---

    async fn fetch_user_disconnect_info(
        &self,
        target_uid: &Uid,
        explicit_session_id: Option<SessionId>,
    ) -> Option<UserDisconnectInfo> {
        let user_arc = self.user_manager.users.get(target_uid)?;
        let user = user_arc.read().await;
        Some(UserDisconnectInfo {
            nick: user.nick.clone(),
            user: user.user.clone(),
            host: user.host.clone(),
            realname: user.realname.clone(),
            channels: user.channels.iter().cloned().collect(),
            session_id: explicit_session_id.unwrap_or(user.session_id),
            account: user.account.clone(),
            is_invisible: user.modes.invisible,
            is_oper: user.modes.oper,
        })
    }

    /// Returns true if session was detached and user should REMAIN connected.
    async fn process_session_detachment(&self, uid: &Uid, info: &UserDisconnectInfo) -> bool {
        use crate::state::managers::client::DetachResult;

        if !self.config.multiclient.enabled {
            return false;
        }

        if info.account.is_none() {
            return false;
        }

        let detach_result = self.client_manager.detach_session(info.session_id).await;

        match detach_result {
            DetachResult::Detached { remaining_sessions } => {
                tracing::debug!(
                    uid = %uid,
                    account = ?info.account,
                    remaining = %remaining_sessions,
                    "Session detached, other sessions remain"
                );
                // CRITICAL: If other sessions remain, the user stays in channels and online.
                true
            }
            DetachResult::Persisting => {
                tracing::info!(
                    uid = %uid,
                    account = ?info.account,
                    "Session detached, client persisting (always-on)"
                );

                // Set auto-away on the User instead of removing
                if let Some(account) = &info.account
                    && let Some(client) = self.client_manager.get_client(account)
                {
                    let client_guard = client.read().await;
                    if let Some(away_msg) = &client_guard.away
                        && let Some(user_arc) = self.user_manager.users.get(uid)
                    {
                        let mut user = user_arc.write().await;
                        user.away = Some(away_msg.clone());
                        tracing::debug!(
                            uid = %uid,
                            away = %away_msg,
                            "Set auto-away on always-on user"
                        );
                    }
                }
                false
            }
            DetachResult::Destroyed => {
                tracing::debug!(
                    uid = %uid,
                    account = ?info.account,
                    "Session detached, client destroyed"
                );
                false
            }
            DetachResult::NotFound => false,
        }
    }

    async fn cleanup_monitors_and_whowas(self: &Arc<Self>, uid: &Uid, info: &UserDisconnectInfo) {
        // Clean up this user's MONITOR entries
        cleanup_monitors(self, uid.as_str());

        // Record WHOWAS entry
        self.user_manager
            .record_whowas(&info.nick, &info.user, &info.host, &info.realname);

        // Notify MONITOR watchers
        notify_monitors_offline(self, &info.nick).await;
    }

    async fn broadcast_quit_and_leave_channels(
        &self,
        uid: &Uid,
        info: &UserDisconnectInfo,
        reason: &str,
    ) {
        use slirc_proto::{Command, Prefix};

        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                info.nick.clone(),
                info.user.clone(),
                info.host.clone(),
            )),
            command: Command::QUIT(Some(reason.to_string())),
        };

        for channel_name in &info.channels {
            let channel_tx = self
                .channel_manager
                .channels
                .get(channel_name)
                .map(|s| s.value().clone());
            if let Some(channel_tx) = channel_tx {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = channel_tx
                    .send(ChannelEvent::Quit {
                        uid: uid.clone(),
                        quit_msg: quit_msg.clone(),
                        reply_tx: Some(tx),
                    })
                    .await;

                if let Ok(remaining) = rx.await
                    && remaining == 0
                    && self.channel_manager.channels.remove(channel_name).is_some()
                {
                    crate::metrics::dec_active_channels();
                    self.stats_manager.channel_destroyed();
                }
            }
        }
    }

    async fn cleanup_user_state(&self, uid: &Uid, info: &UserDisconnectInfo) {
        // Remove from nick mapping
        let nick_lower = slirc_proto::irc_to_lower(&info.nick);
        if let Some(mut vec) = self.user_manager.nicks.get_mut(&nick_lower) {
            vec.retain(|u| u != uid);
            if vec.is_empty() {
                drop(vec);
                self.user_manager.nicks.remove(&nick_lower);
            }
        }

        // Remove user from matrix
        self.user_manager.users.remove(uid);

        // Remove enforcement timer
        self.user_manager.enforce_timers.remove(uid);

        // Drop sender
        self.user_manager.senders.remove(uid);

        // Clean up rate limiter
        self.security_manager.rate_limiter.remove_client(uid);

        // Update metrics
        crate::metrics::dec_connected_users();

        // Update StatsManager
        if uid.starts_with(self.server_id.as_str()) {
            self.stats_manager.user_disconnected();
            if info.is_invisible {
                self.stats_manager.user_unset_invisible();
            }
            if info.is_oper {
                self.stats_manager.user_deopered();
            }
        } else {
            self.stats_manager.remote_user_disconnected();
            if info.is_oper {
                self.stats_manager.remote_user_deopered();
            }
        }
    }
}
