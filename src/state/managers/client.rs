//! Client manager for bouncer/multiclient support.
//!
//! The ClientManager handles:
//! - Creating and tracking Client instances per account
//! - Session attachment and detachment
//! - Always-on client lifecycle management
//!
//! # Thread Safety
//!
//! All operations are thread-safe via DashMap. The lock order follows
//! Matrix conventions: DashMap shard lock → Client RwLock.

use crate::state::client::{Client, DeviceId, SessionAttachment, SessionId};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tokio::sync::RwLock;

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
        }
    }

    /// Create a new ClientManager with custom settings.
    pub fn with_max_sessions(max_sessions: usize) -> Self {
        Self {
            clients: DashMap::new(),
            session_to_client: DashMap::new(),
            session_info: DashMap::new(),
            max_sessions_per_account: max_sessions,
        }
    }

    /// Get a client by account name.
    pub fn get_client(&self, account: &str) -> Option<Arc<RwLock<Client>>> {
        let account_lower = irc_to_lower(account);
        self.clients.get(&account_lower).map(|c| c.value().clone())
    }

    /// Get a client by session ID.
    pub fn get_client_by_session(&self, session_id: &SessionId) -> Option<Arc<RwLock<Client>>> {
        self.session_to_client
            .get(session_id)
            .map(|c| c.value().clone())
    }

    /// Get session attachment info.
    pub fn get_session_info(&self, session_id: &SessionId) -> Option<SessionAttachment> {
        self.session_info.get(session_id).map(|s| s.value().clone())
    }

    /// Get or create a client for an account.
    ///
    /// If the client doesn't exist, creates a new one with the given nick.
    pub async fn get_or_create_client(
        &self,
        account: &str,
        nick: &str,
    ) -> Arc<RwLock<Client>> {
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
    pub async fn attach_session(
        &self,
        account: &str,
        nick: &str,
        session_id: SessionId,
        device_id: Option<DeviceId>,
        ip: String,
        multiclient_allowed: bool,
    ) -> AttachResult {
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

            // Update last-seen for device
            if let Some(ref device) = device_id {
                client_guard.update_last_seen(device);
                client_guard.register_device(device.clone(), None);
            }

            // Update nick in case it changed
            client_guard.nick = nick.to_string();
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
        self.session_info.remove(&session_id);

        // Detach from client
        let (remaining, always_on);
        {
            let mut client_guard = client.write().await;
            client_guard.detach_session(session_id);
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
            DetachResult::Destroyed
        }
    }

    /// Check if a client is connected (has any sessions).
    pub async fn is_connected(&self, account: &str) -> bool {
        let account_lower = irc_to_lower(account);
        if let Some(client) = self.clients.get(&account_lower) {
            let client_guard = client.read().await;
            client_guard.is_connected()
        } else {
            false
        }
    }

    /// Get the number of sessions for an account.
    pub async fn session_count(&self, account: &str) -> usize {
        let account_lower = irc_to_lower(account);
        if let Some(client) = self.clients.get(&account_lower) {
            let client_guard = client.read().await;
            client_guard.session_count()
        } else {
            0
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

    /// Get the current nick for an account (if client exists).
    pub async fn get_nick(&self, account: &str) -> Option<String> {
        let account_lower = irc_to_lower(account);
        if let Some(client) = self.clients.get(&account_lower) {
            let client_guard = client.read().await;
            Some(client_guard.nick.clone())
        } else {
            None
        }
    }

    /// Update the nick for a client.
    pub async fn update_nick(&self, account: &str, new_nick: &str) {
        let account_lower = irc_to_lower(account);
        if let Some(client) = self.clients.get(&account_lower) {
            let mut client_guard = client.write().await;
            client_guard.nick = new_nick.to_string();
        }
    }

    /// List all always-on clients (for persistence/restoration).
    pub fn always_on_clients(&self) -> Vec<Arc<RwLock<Client>>> {
        self.clients
            .iter()
            .map(|c| c.value().clone())
            .collect()
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
                    client_guard
                        .last_seen
                        .values()
                        .all(|&ts| ts < cutoff)
                }
            };

            if should_expire {
                self.clients.remove(&account);
                expired.push(account);
            }
        }

        expired
    }

    /// Get statistics about the client manager.
    pub async fn stats(&self) -> ClientManagerStats {
        let total_clients = self.clients.len();
        let total_sessions = self.session_to_client.len();

        let mut connected_clients = 0;
        let mut always_on_clients = 0;
        let mut disconnected_always_on = 0;

        let clients: Vec<_> = self.clients.iter().map(|c| c.value().clone()).collect();
        for client in clients {
            let guard = client.read().await;
            if guard.is_connected() {
                connected_clients += 1;
            }
            if guard.always_on {
                always_on_clients += 1;
                if !guard.is_connected() {
                    disconnected_always_on += 1;
                }
            }
        }

        ClientManagerStats {
            total_clients,
            connected_clients,
            always_on_clients,
            disconnected_always_on,
            total_sessions,
        }
    }
}

impl Default for ClientManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the client manager.
#[derive(Debug, Clone)]
pub struct ClientManagerStats {
    /// Total number of clients (accounts with bouncer state).
    pub total_clients: usize,
    /// Clients with at least one connected session.
    pub connected_clients: usize,
    /// Clients with always-on enabled.
    pub always_on_clients: usize,
    /// Always-on clients with no connected sessions.
    pub disconnected_always_on: usize,
    /// Total number of sessions across all clients.
    pub total_sessions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_attach() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        let result = manager
            .attach_session("Alice", "Alice", session, None, "127.0.0.1".to_string(), true)
            .await;

        assert!(matches!(result, AttachResult::Created));
        assert!(manager.is_connected("alice").await);
        assert_eq!(manager.session_count("alice").await, 1);
    }

    #[tokio::test]
    async fn multiclient_attach() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // First session creates client
        let r1 = manager
            .attach_session("Alice", "Alice", session1, None, "127.0.0.1".to_string(), true)
            .await;
        assert!(matches!(r1, AttachResult::Created));

        // Second session attaches to existing client
        let r2 = manager
            .attach_session("Alice", "Alice", session2, None, "127.0.0.2".to_string(), true)
            .await;
        assert!(matches!(
            r2,
            AttachResult::Attached {
                reattach: false,
                first_session: false
            }
        ));
        assert_eq!(manager.session_count("alice").await, 2);
    }

    #[tokio::test]
    async fn multiclient_denied() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // First session creates client
        manager
            .attach_session("Alice", "Alice", session1, None, "127.0.0.1".to_string(), true)
            .await;

        // Second session denied when multiclient disabled
        let r2 = manager
            .attach_session(
                "Alice",
                "Alice",
                session2,
                None,
                "127.0.0.2".to_string(),
                false,
            )
            .await;
        assert!(matches!(r2, AttachResult::MulticlientNotAllowed));
        assert_eq!(manager.session_count("alice").await, 1);
    }

    #[tokio::test]
    async fn detach_and_destroy() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        manager
            .attach_session("Alice", "Alice", session, None, "127.0.0.1".to_string(), true)
            .await;

        let result = manager.detach_session(session).await;
        assert!(matches!(result, DetachResult::Destroyed));
        assert!(!manager.is_connected("alice").await);
        assert!(manager.get_client("alice").is_none());
    }

    #[tokio::test]
    async fn detach_with_always_on() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        // Create client and enable always-on
        manager
            .attach_session("Alice", "Alice", session, None, "127.0.0.1".to_string(), true)
            .await;
        {
            let client = manager.get_client("alice").unwrap();
            client.write().await.set_always_on(true);
        }

        // Detach - should persist
        let result = manager.detach_session(session).await;
        assert!(matches!(result, DetachResult::Persisting));
        assert!(!manager.is_connected("alice").await);
        assert!(manager.get_client("alice").is_some());
    }

    #[tokio::test]
    async fn reattach_to_always_on() {
        let manager = ClientManager::new();
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // Create client with always-on
        manager
            .attach_session("Alice", "Alice", session1, None, "127.0.0.1".to_string(), true)
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
            .attach_session(
                "Alice",
                "Alice",
                session2,
                None,
                "127.0.0.2".to_string(),
                true,
            )
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
            .attach_session("Alice", "Alice", s1, None, "1".to_string(), true)
            .await;
        manager
            .attach_session("Alice", "Alice", s2, None, "2".to_string(), true)
            .await;

        let result = manager
            .attach_session("Alice", "Alice", s3, None, "3".to_string(), true)
            .await;
        assert!(matches!(result, AttachResult::TooManySessions));
    }

    #[tokio::test]
    async fn device_tracking() {
        let manager = ClientManager::new();
        let session = SessionId::new_v4();

        manager
            .attach_session(
                "Alice",
                "Alice",
                session,
                Some("phone".to_string()),
                "127.0.0.1".to_string(),
                true,
            )
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
            .attach_session(
                "Alice",
                "Alice",
                s1,
                Some("phone".to_string()),
                "1".to_string(),
                true,
            )
            .await;
        manager
            .attach_session(
                "Alice",
                "Alice",
                s2,
                Some("laptop".to_string()),
                "2".to_string(),
                true,
            )
            .await;

        let sessions = manager.get_sessions("alice");
        assert_eq!(sessions.len(), 2);
        let device_ids: Vec<_> = sessions.iter().filter_map(|s| s.device_id.clone()).collect();
        assert!(device_ids.contains(&"phone".to_string()));
        assert!(device_ids.contains(&"laptop".to_string()));
    }

    #[tokio::test]
    async fn stats() {
        let manager = ClientManager::new();
        let s1 = SessionId::new_v4();
        let s2 = SessionId::new_v4();

        manager
            .attach_session("Alice", "Alice", s1, None, "1".to_string(), true)
            .await;
        manager
            .attach_session("Bob", "Bob", s2, None, "2".to_string(), true)
            .await;

        // Set Bob to always-on and disconnect
        {
            let client = manager.get_client("bob").unwrap();
            client.write().await.set_always_on(true);
        }
        manager.detach_session(s2).await;

        let stats = manager.stats().await;
        assert_eq!(stats.total_clients, 2);
        assert_eq!(stats.connected_clients, 1);
        assert_eq!(stats.always_on_clients, 1);
        assert_eq!(stats.disconnected_always_on, 1);
        assert_eq!(stats.total_sessions, 1);
    }
}
