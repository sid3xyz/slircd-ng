//! Client manager for bouncer/multiclient support.
//!
//! The ClientManager handles:
//! - Creating and tracking Client instances per account
//! - Session attachment and detachment
//! - Always-on client lifecycle management
//! - Dirty-bit writeback to persistent storage
//!
//! # Thread Safety
//!
//! All operations are thread-safe via DashMap. The lock order follows
//! Matrix conventions: DashMap shard lock → Client RwLock.

use crate::db::always_on::AlwaysOnStore;
use crate::state::MemberModes;
use crate::state::client::{Client, DeviceId, SessionAttachment, SessionId};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Manages all Client instances for bouncer functionality.
pub struct ClientManager {
    /// Clients by account name (casefolded).
    clients: DashMap<String, Arc<RwLock<Client>>>,

    /// Session ID to Client mapping for fast lookup.
    session_to_client: DashMap<SessionId, Arc<RwLock<Client>>>,

    /// Session ID to attachment info.
    session_info: DashMap<SessionId, SessionAttachment>,

    /// Maximum sessions per account (DoS protection).
    max_sessions_per_account: usize,

    /// Persistent storage for always-on clients (optional).
    store: Option<Arc<AlwaysOnStore>>,
}

/// Result of attempting to attach a session.
#[derive(Debug)]
pub enum AttachResult {
    /// Successfully attached to an existing client.
    Attached {
        /// Whether this was a reattach (client had previous sessions).
        reattach: bool,
        /// Whether this is the first session (triggers welcome burst).
        first_session: bool,
    },
    /// Created a new client and attached.
    Created,
    /// Multiclient not allowed (another session exists and multiclient disabled).
    MulticlientNotAllowed,
    /// Too many sessions on this account.
    TooManySessions,
}

/// Parameters for attaching a session to a client.
#[derive(Debug)]
pub struct AttachSessionRequest<'a> {
    pub account: &'a str,
    pub nick: &'a str,
    pub session_id: SessionId,
    pub device_id: Option<DeviceId>,
    pub ip: String,
    pub multiclient_allowed: bool,
    pub always_on_enabled: bool,
    pub auto_away_enabled: bool,
}

/// Result of detaching a session.
#[derive(Debug)]
pub enum DetachResult {
    /// Session detached, client still has other sessions.
    Detached { remaining_sessions: usize },
    /// Session detached, client is now disconnected but persisting (always-on).
    Persisting,
    /// Session detached, client has been destroyed (no always-on).
    Destroyed,
    /// Session was not attached to any client.
    NotFound,
}

impl ClientManager {
    /// Create a new ClientManager with default settings.
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            session_to_client: DashMap::new(),
            session_info: DashMap::new(),
            max_sessions_per_account: 10,
            store: None,
        }
    }

    /// Create a new ClientManager with custom settings.
    pub fn with_max_sessions(max_sessions: usize) -> Self {
        Self {
            clients: DashMap::new(),
            session_to_client: DashMap::new(),
            session_info: DashMap::new(),
            max_sessions_per_account: max_sessions,
            store: None,
        }
    }

    /// Create a new ClientManager with persistence.
    pub fn with_store(store: Arc<AlwaysOnStore>, max_sessions: usize) -> Self {
        Self {
            clients: DashMap::new(),
            session_to_client: DashMap::new(),
            session_info: DashMap::new(),
            max_sessions_per_account: max_sessions,
            store: Some(store),
        }
    }

    /// Get a client by account name.
    pub fn get_client(&self, account: &str) -> Option<Arc<RwLock<Client>>> {
        let account_lower = irc_to_lower(account);
        self.clients.get(&account_lower).map(|c| c.value().clone())
    }

    /// Get or create a client for an account.
    ///
    /// If the client doesn't exist, creates a new one with the given nick.
    pub async fn get_or_create_client(&self, account: &str, nick: &str) -> Arc<RwLock<Client>> {
        let account_lower = irc_to_lower(account);

        // Try to get existing client first
        if let Some(client) = self.clients.get(&account_lower) {
            return client.value().clone();
        }

        // Create new client
        let client = Arc::new(RwLock::new(Client::new(
            account_lower.clone(),
            nick.to_string(),
        )));

        // Insert with race protection
        self.clients
            .entry(account_lower)
            .or_insert(client.clone())
            .value()
            .clone()
    }

    /// Attach a session to a client.
    ///
    /// If `multiclient_allowed` is false and the client already has sessions,
    /// returns `MulticlientNotAllowed`.
    pub async fn attach_session(&self, request: AttachSessionRequest<'_>) -> AttachResult {
        let AttachSessionRequest {
            account,
            nick,
            session_id,
            device_id,
            ip,
            multiclient_allowed,
            always_on_enabled,
            auto_away_enabled,
        } = request;

        let account_lower = irc_to_lower(account);

        // Get or create client
        let client = self.get_or_create_client(account, nick).await;

        // Check session limit and multiclient policy
        {
            let client_guard = client.read().await;
            let current_sessions = client_guard.session_count();

            if current_sessions > 0 && !multiclient_allowed {
                return AttachResult::MulticlientNotAllowed;
            }

            if current_sessions >= self.max_sessions_per_account {
                return AttachResult::TooManySessions;
            }
        }

        // Attach the session
        let (was_new_client, was_first_session);
        {
            let mut client_guard = client.write().await;
            was_first_session = !client_guard.is_connected();
            was_new_client = client_guard.channels.is_empty() && was_first_session;
            client_guard.attach_session(session_id);

            // Apply policy flags from configuration and account defaults
            client_guard.set_always_on(always_on_enabled);
            client_guard.set_auto_away(auto_away_enabled);

            // Update last-seen for device
            if let Some(ref device) = device_id {
                client_guard.update_last_seen(device);
                client_guard.register_device(device.clone(), None);
                client_guard.touch_device(device);
            }

            // Update nick in case it changed
            if client_guard.nick != nick {
                client_guard.nick = nick.to_string();
                client_guard.mark_dirty(crate::state::client::dirty::NICK);
            }
        }

        // Record session mapping
        self.session_to_client.insert(session_id, client.clone());
        self.session_info.insert(
            session_id,
            SessionAttachment {
                session_id,
                device_id,
                account: account_lower,
                ip,
                attached_at: Utc::now(),
            },
        );

        if was_new_client {
            AttachResult::Created
        } else {
            AttachResult::Attached {
                reattach: was_first_session,
                first_session: was_first_session,
            }
        }
    }

    /// Detach a session from its client.
    pub async fn detach_session(&self, session_id: SessionId) -> DetachResult {
        // Remove session mappings
        let client = match self.session_to_client.remove(&session_id) {
            Some((_, client)) => client,
            None => return DetachResult::NotFound,
        };
        let session_info = self.session_info.remove(&session_id).map(|(_, info)| info);

        // Detach from client
        let (remaining, always_on);
        {
            let mut client_guard = client.write().await;
            client_guard.detach_session(session_id);
            if let Some(info) = &session_info
                && let Some(device_id) = &info.device_id
            {
                client_guard.update_last_seen(device_id);
                client_guard.touch_device(device_id);
            }
            remaining = client_guard.session_count();
            always_on = client_guard.always_on;
        }

        if remaining > 0 {
            DetachResult::Detached {
                remaining_sessions: remaining,
            }
        } else if always_on {
            DetachResult::Persisting
        } else {
            // Destroy the client
            let account_lower = {
                let client_guard = client.read().await;
                client_guard.account.clone()
            };
            self.clients.remove(&account_lower);
            if let Some(store) = &self.store
                && let Err(e) = store.delete_client(&account_lower)
            {
                error!(account = %account_lower, error = %e, "Failed to delete stored client");
            }
            DetachResult::Destroyed
        }
    }

    /// Get all sessions for an account.
    pub fn get_sessions(&self, account: &str) -> Vec<SessionAttachment> {
        let account_lower = irc_to_lower(account);
        self.session_info
            .iter()
            .filter(|s| s.value().account == account_lower)
            .map(|s| s.value().clone())
            .collect()
    }

    /// Track a successful channel join for an account-backed client.
    pub async fn record_channel_join(
        &self,
        account: &str,
        channel: &str,
        member_modes: Option<&MemberModes>,
    ) {
        let Some(client) = self.get_client(account) else {
            return;
        };

        let mut client_guard = client.write().await;
        let modes = member_modes.map(member_modes_to_string).unwrap_or_default();
        client_guard.join_channel(channel, &modes);

        if let Some(join_time) = member_modes.and_then(|m| m.join_time) {
            let channel_lower = irc_to_lower(channel);
            if let Some(membership) = client_guard.channels.get_mut(&channel_lower) {
                membership.join_time = join_time;
                client_guard.mark_dirty(crate::state::client::dirty::CHANNELS);
            }
        }
    }

    /// Track a channel part (PART/KICK) for an account-backed client.
    pub async fn record_channel_part(&self, account: &str, channel: &str) {
        let Some(client) = self.get_client(account) else {
            return;
        };
        let mut client_guard = client.write().await;
        client_guard.part_channel(channel);
    }

    /// Update the nick for a client.
    pub async fn update_nick(&self, account: &str, new_nick: &str) {
        let account_lower = irc_to_lower(account);
        if let Some(client) = self.clients.get(&account_lower) {
            let mut client_guard = client.write().await;
            if client_guard.nick != new_nick {
                client_guard.nick = new_nick.to_string();
                client_guard.mark_dirty(crate::state::client::dirty::NICK);
            }
        }
    }

    /// Expire disconnected always-on clients older than the given duration.
    pub async fn expire_clients(&self, max_age: Duration) -> Vec<String> {
        let cutoff = Utc::now() - max_age;
        let mut expired = Vec::new();

        // Collect candidates first to avoid holding locks during removal
        let candidates: Vec<(String, Arc<RwLock<Client>>)> = self
            .clients
            .iter()
            .map(|c| (c.key().clone(), c.value().clone()))
            .collect();

        for (account, client) in candidates {
            let should_expire = {
                let client_guard = client.read().await;
                // Only expire if:
                // 1. No sessions connected
                // 2. All devices haven't been seen since cutoff
                if client_guard.is_connected() {
                    false
                } else if client_guard.last_seen.is_empty() {
                    // No devices, check created_at
                    client_guard.created_at < cutoff
                } else {
                    // All devices must be stale
                    client_guard.last_seen.values().all(|&ts| ts < cutoff)
                }
            };

            if should_expire {
                self.clients.remove(&account);
                expired.push(account);
            }
        }

        expired
    }

    // =========================================================================
    // Persistence Methods
    // =========================================================================

    /// Persist a client's state to storage (if store is configured).
    pub async fn persist_client(&self, account: &str) {
        if let Some(store) = &self.store {
            let account_lower = irc_to_lower(account);
            if let Some(client_arc) = self.clients.get(&account_lower) {
                let client = client_arc.read().await;
                if client.always_on {
                    if let Err(e) = store.save_client(&client) {
                        error!(account = %account, error = %e, "Failed to persist client");
                    } else {
                        debug!(account = %account, "Persisted client state");
                    }
                }
            }
        }
    }

    /// Perform dirty-bit writeback for all clients.
    ///
    /// This should be called periodically (e.g., every 30 seconds).
    pub async fn writeback_dirty(&self) -> usize {
        let Some(store) = &self.store else {
            return 0;
        };

        let mut written = 0;
        let clients: Vec<(String, Arc<RwLock<Client>>)> = self
            .clients
            .iter()
            .map(|c| (c.key().clone(), c.value().clone()))
            .collect();

        for (account, client_arc) in clients {
            let (should_persist, dirty_bits) = {
                let client = client_arc.read().await;
                let dirty = client.take_dirty();
                (client.always_on && dirty != 0, dirty)
            };

            if should_persist {
                let client = client_arc.read().await;
                if let Err(e) = store.save_client(&client) {
                    error!(account = %account, error = %e, "Failed to writeback client");
                } else {
                    debug!(account = %account, dirty_bits = dirty_bits, "Writeback complete");
                    written += 1;
                }
            }
        }

        if written > 0 {
            info!(count = written, "Completed dirty-bit writeback");
        }
        written
    }

    /// Restore always-on clients from persistent storage.
    ///
    /// Returns the number of clients restored.
    pub async fn restore_from_storage(&self) -> Result<usize, crate::db::AlwaysOnError> {
        let Some(store) = &self.store else {
            return Ok(0);
        };

        let stored_clients = store.load_all_clients()?;
        let mut restored = 0;

        for stored in stored_clients {
            let mut client = stored.to_client();

            // Restore device metadata
            for device in store.load_devices(&stored.account)? {
                client.devices.insert(device.id.clone(), device.to_device());
            }

            let account_lower = irc_to_lower(&client.account);

            // Only restore if not already loaded
            if !self.clients.contains_key(&account_lower) {
                self.clients
                    .insert(account_lower.clone(), Arc::new(RwLock::new(client)));
                restored += 1;
                debug!(account = %stored.account, "Restored always-on client");
            }
        }

        info!(count = restored, "Restored always-on clients from storage");
        Ok(restored)
    }

    /// Expire stale clients from persistent storage.
    pub fn expire_from_storage(
        &self,
        cutoff: chrono::DateTime<Utc>,
    ) -> Result<Vec<String>, crate::db::AlwaysOnError> {
        if let Some(store) = &self.store {
            store.expire_clients(cutoff)
        } else {
            Ok(Vec::new())
        }
    }
}

impl Default for ClientManager {
    fn default() -> Self {
        Self::new()
    }
}
fn member_modes_to_string(modes: &MemberModes) -> String {
    let mut out = String::new();
    if modes.owner {
        out.push('q');
    }
    if modes.admin {
        out.push('a');
    }
    if modes.op {
        out.push('o');
    }
    if modes.halfop {
        out.push('h');
    }
    if modes.voice {
        out.push('v');
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    fn attach_request<'a>(
        account: &'a str,
        nick: &'a str,
        session_id: SessionId,
        device_id: Option<DeviceId>,
        ip: &str,
        multiclient_allowed: bool,
    ) -> AttachSessionRequest<'a> {
        AttachSessionRequest {
            account,
            nick,
            session_id,
            device_id,
            ip: ip.to_string(),
            multiclient_allowed,
            always_on_enabled: false,
            auto_away_enabled: false,
        }
    }

    async fn is_connected(manager: &ClientManager, account: &str) -> bool {
        let account_lower = irc_to_lower(account);
        let client = manager
            .clients
            .get(&account_lower)
            .map(|c| c.value().clone());
        if let Some(client) = client {
            let guard = client.read().await;
            guard.is_connected()
        } else {
            false
        }
    }

    async fn session_count(manager: &ClientManager, account: &str) -> usize {
        let account_lower = irc_to_lower(account);
        let client = manager
            .clients
            .get(&account_lower)
            .map(|c| c.value().clone());
        if let Some(client) = client {
            let guard = client.read().await;
            guard.session_count()
        } else {
            0
        }
    }

    #[tokio::test]
    async fn create_and_attach() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        let result = manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session,
                None,
                "127.0.0.1",
                true,
            ))
            .await;

        assert!(matches!(result, AttachResult::Created));
        assert!(is_connected(&manager, "alice").await);
        assert_eq!(session_count(&manager, "alice").await, 1);
    }

    #[tokio::test]
    async fn multiclient_attach() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // First session creates client
        let r1 = manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session1,
                None,
                "127.0.0.1",
                true,
            ))
            .await;
        assert!(matches!(r1, AttachResult::Created));

        // Second session attaches to existing client
        let r2 = manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session2,
                None,
                "127.0.0.2",
                true,
            ))
            .await;
        assert!(matches!(
            r2,
            AttachResult::Attached {
                reattach: false,
                first_session: false
            }
        ));
        assert_eq!(session_count(&manager, "alice").await, 2);
    }

    #[tokio::test]
    async fn multiclient_denied() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // First session creates client
        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session1,
                None,
                "127.0.0.1",
                true,
            ))
            .await;

        // Second session denied when multiclient disabled
        let r2 = manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session2,
                None,
                "127.0.0.2",
                false,
            ))
            .await;
        assert!(matches!(r2, AttachResult::MulticlientNotAllowed));
        assert_eq!(session_count(&manager, "alice").await, 1);
    }

    #[tokio::test]
    async fn detach_and_destroy() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session,
                None,
                "127.0.0.1",
                true,
            ))
            .await;

        let result = manager.detach_session(session).await;
        assert!(matches!(result, DetachResult::Destroyed));
        assert!(!is_connected(&manager, "alice").await);
        assert!(manager.get_client("alice").is_none());
    }

    #[tokio::test]
    async fn detach_with_always_on() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        // Create client and enable always-on
        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session,
                None,
                "127.0.0.1",
                true,
            ))
            .await;
        {
            let client = manager.get_client("alice").unwrap();
            client.write().await.set_always_on(true);
        }

        // Detach - should persist
        let result = manager.detach_session(session).await;
        assert!(matches!(result, DetachResult::Persisting));
        assert!(!is_connected(&manager, "alice").await);
        assert!(manager.get_client("alice").is_some());
    }

    #[tokio::test]
    async fn reattach_to_always_on() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // Create client with always-on
        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session1,
                None,
                "127.0.0.1",
                true,
            ))
            .await;
        {
            let client = manager.get_client("alice").unwrap();
            let mut guard = client.write().await;
            guard.set_always_on(true);
            guard.join_channel("#test", "o");
        }

        // Detach
        manager.detach_session(session1).await;

        // Reattach
        let result = manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session2,
                None,
                "127.0.0.2",
                true,
            ))
            .await;
        assert!(matches!(
            result,
            AttachResult::Attached {
                reattach: true,
                first_session: true
            }
        ));

        // Verify channels are preserved
        let client = manager.get_client("alice").unwrap();
        let guard = client.read().await;
        assert!(guard.channels.contains_key("#test"));
    }

    #[tokio::test]
    async fn session_limit() {
        let manager = ClientManager::with_max_sessions(2);
        let s1 = SessionId::new_v4();
        let s2 = SessionId::new_v4();
        let s3 = SessionId::new_v4();

        manager
            .attach_session(attach_request("Alice", "Alice", s1, None, "1", true))
            .await;
        manager
            .attach_session(attach_request("Alice", "Alice", s2, None, "2", true))
            .await;

        let result = manager
            .attach_session(attach_request("Alice", "Alice", s3, None, "3", true))
            .await;
        assert!(matches!(result, AttachResult::TooManySessions));
    }

    #[tokio::test]
    async fn device_tracking() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                session,
                Some("phone".to_string()),
                "127.0.0.1",
                true,
            ))
            .await;

        let client = manager.get_client("alice").unwrap();
        let guard = client.read().await;
        assert!(guard.devices.contains_key("phone"));
        assert!(guard.last_seen.contains_key("phone"));
    }

    #[tokio::test]
    async fn get_sessions() {
        let manager = ClientManager::new();
        let s1 = SessionId::new_v4();
        let s2 = SessionId::new_v4();

        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                s1,
                Some("phone".to_string()),
                "1",
                true,
            ))
            .await;
        manager
            .attach_session(attach_request(
                "Alice",
                "Alice",
                s2,
                Some("laptop".to_string()),
                "2",
                true,
            ))
            .await;

        let sessions = manager.get_sessions("alice");
        assert_eq!(sessions.len(), 2);
        let device_ids: Vec<_> = sessions
            .iter()
            .filter_map(|s| s.device_id.clone())
            .collect();
        assert!(device_ids.contains(&"phone".to_string()));
        assert!(device_ids.contains(&"laptop".to_string()));
    }

    #[tokio::test]
    async fn counts_after_detach() {
        let manager = ClientManager::new();
        let s1 = SessionId::new_v4();
        let s2 = SessionId::new_v4();

        manager
            .attach_session(attach_request("Alice", "Alice", s1, None, "1", true))
            .await;
        manager
            .attach_session(attach_request("Bob", "Bob", s2, None, "2", true))
            .await;

        // Set Bob to always-on and disconnect
        {
            let client = manager.get_client("bob").unwrap();
            client.write().await.set_always_on(true);
        }
        manager.detach_session(s2).await;

        assert_eq!(session_count(&manager, "alice").await, 1);
        assert_eq!(session_count(&manager, "bob").await, 0);
    }
}
