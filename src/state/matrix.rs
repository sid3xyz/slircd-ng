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
use crate::state::{
    ChannelManager, LifecycleManager, MonitorManager, SecurityManager, SecurityManagerParams,
    ServiceManager, SyncManager, Uid, UserManager,
};
use slirc_crdt::clock::ServerId;

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

    /// This server's identity.
    pub server_info: ServerInfo,

    /// Server ID for CRDT synchronization.
    pub server_id: ServerId,

    /// Server configuration (for handlers to access).
    pub config: MatrixConfig,

    /// Router channel for remote messages.
    pub router_tx: mpsc::Sender<Arc<Message>>,
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
    /// Command output limits (WHO, LIST, NAMES).
    pub limits: crate::config::LimitsConfig,
    /// History configuration (Innovation 5: Event-Sourced History).
    pub history: crate::config::HistoryConfig,
    /// Link blocks for server peering.
    pub links: Vec<crate::config::LinkBlock>,
    /// TLS configuration (for STS capability advertising).
    pub tls: Option<crate::config::TlsConfig>,
}

/// This server's identity information.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub network: String,
    pub sid: String,
    pub description: String,
    pub created: i64,
    /// MOTD lines loaded from config file.
    pub motd_lines: Vec<String>,
    /// Idle timeout configuration for ping/pong keepalive.
    pub idle_timeouts: crate::config::IdleTimeoutsConfig,
}

/// Parameters for creating a new Matrix.
pub struct MatrixParams<'a> {
    pub config: &'a Config,
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
}

impl Matrix {
    /// Create a new Matrix with the given server configuration.
    pub fn new(params: MatrixParams<'_>) -> (Self, mpsc::Receiver<Arc<Message>>) {
        let MatrixParams {
            config,
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
        );
        let sync_manager_arc = Arc::new(sync_manager);
        let mut user_manager =
            UserManager::new(config.server.sid.clone(), config.server.name.clone());
        user_manager.set_observer(sync_manager_arc.clone());

        let mut channel_manager =
            ChannelManager::with_registered_channels(registered_channel_names);
        channel_manager.set_observer(sync_manager_arc.clone());

        // Create ServiceManager with server SID for service UIDs
        let service_manager = ServiceManager::new(db.clone(), history, &config.server.sid);

        // Register service pseudoclients in UserManager
        let service_users = service_manager.create_service_users(&config.server.name, &server_id);
        for user in service_users {
            user_manager.register_service_user(user);
        }

        let (router_tx, router_rx) = mpsc::channel(1000);

        (
            Self {
                user_manager,
                channel_manager,
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
                    limits: config.limits.clone(),
                    history: config.history.clone(),
                    links: config.links.clone(),
                    tls: config.tls.clone(),
                },
                router_tx,
            },
            router_rx,
        )
    }

    /// Register a user's message sender for routing.
    pub fn register_sender(&self, uid: &str, sender: mpsc::Sender<Arc<Message>>) {
        self.user_manager.senders.insert(uid.to_string(), sender);
    }

    /// Get the current hybrid timestamp for CRDT operations.
    pub fn clock(&self) -> slirc_crdt::clock::HybridTimestamp {
        slirc_crdt::clock::HybridTimestamp::now(&self.server_id)
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
        use slirc_proto::{Command, Prefix};

        // Get user info before removal
        let (nick, user, host, realname, user_channels) = {
            let user_arc = self.user_manager.users.get(target_uid).map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
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
        self.user_manager
            .record_whowas(&nick, &user, &host, &realname);

        // Notify MONITOR watchers that this nick is going offline
        notify_monitors_offline(self, &nick).await;

        // Clean up this user's MONITOR entries
        cleanup_monitors(self, target_uid.as_str());

        // Build QUIT message
        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(nick.clone(), user, host)),
            command: Command::QUIT(Some(quit_reason.to_string())),
        };

        // Remove from channels and broadcast QUIT
        for channel_name in &user_channels {
            let channel_tx = self
                .channel_manager
                .channels
                .get(channel_name)
                .map(|s| s.clone());
            if let Some(channel_tx) = channel_tx {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = channel_tx
                    .send(ChannelEvent::Quit {
                        uid: target_uid.clone(),
                        quit_msg: quit_msg.clone(),
                        reply_tx: Some(tx),
                    })
                    .await;

                if let Ok(remaining) = rx.await
                    && remaining == 0
                    && self.channel_manager.channels.remove(channel_name).is_some()
                {
                    crate::metrics::ACTIVE_CHANNELS.dec();
                }
            }
        }

        // Remove from nick mapping
        let nick_lower = slirc_proto::irc_to_lower(&nick);
        self.user_manager.nicks.remove(&nick_lower);

        // Remove user from matrix
        self.user_manager.users.remove(target_uid);

        // Remove enforcement timer if any
        self.user_manager.enforce_timers.remove(target_uid);

        // Drop sender - this will cause the connection task to terminate
        self.user_manager.senders.remove(target_uid);

        // Clean up rate limiter state
        self.security_manager.rate_limiter.remove_client(target_uid);

        // Update connected user metric
        crate::metrics::CONNECTED_USERS.dec();

        user_channels
    }
}
