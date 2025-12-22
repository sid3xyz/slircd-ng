//! User management state and behavior.
//!
//! This module contains the `UserManager` struct, which isolates all
//! user-related state and logic from the main Matrix struct.

use crate::state::{Uid, UidGenerator, User, WhowasEntry, observer::StateObserver};
use dashmap::DashMap;
use slirc_crdt::clock::ServerId;
use slirc_proto::{Command, Message, Prefix};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Maximum number of WHOWAS entries to keep per nickname.
const MAX_WHOWAS_PER_NICK: usize = 10;

/// Manages all user-related state and behavior.
///
/// The UserManager is responsible for:
/// - Tracking connected users and their nicknames.
/// - Managing message senders for broadcasting.
/// - Maintaining WHOWAS history for disconnected users.
/// - Generating unique identifiers (UIDs).
/// - Handling server-wide notices (snomasks).
pub struct UserManager {
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub nicks: DashMap<String, Uid>,
    pub senders: DashMap<Uid, mpsc::Sender<Arc<Message>>>,
    pub whowas: DashMap<String, VecDeque<WhowasEntry>>,
    pub uid_gen: UidGenerator,
    pub enforce_timers: DashMap<Uid, Instant>,
    /// This server's name (required for snomask and whowas).
    pub server_name: String,
    /// Maximum local user count (historical peak).
    pub max_local_users: AtomicUsize,
    /// Maximum global user count (historical peak).
    pub max_global_users: AtomicUsize,
    /// Observer for state changes (Innovation 2).
    pub observer: Option<Arc<dyn StateObserver>>,
}

impl UserManager {
    pub fn new(server_sid: String, server_name: String) -> Self {
        Self {
            users: DashMap::new(),
            nicks: DashMap::new(),
            senders: DashMap::new(),
            whowas: DashMap::new(),
            uid_gen: UidGenerator::new(server_sid),
            enforce_timers: DashMap::new(),
            server_name,
            max_local_users: AtomicUsize::new(0),
            max_global_users: AtomicUsize::new(0),
            observer: None,
        }
    }

    /// Set the state observer.
    pub fn set_observer(&mut self, observer: Arc<dyn StateObserver>) {
        self.observer = Some(observer);
    }

    /// Notify the observer of a state change.
    /// `source` is the ServerId that originated the change, or None if local.
    #[allow(clippy::collapsible_if)]
    pub async fn notify_observer(&self, uid: &str, source: Option<ServerId>) {
        if let Some(observer) = &self.observer {
            if let Some(user_arc) = self.users.get(uid) {
                let user = user_arc.read().await;
                let crdt = user.to_crdt();
                observer.on_user_update(&crdt, source);
            }
        }
    }

    /// Export all users as CRDTs for a BURST.
    #[allow(dead_code)]
    pub async fn to_crdt(&self) -> Vec<slirc_crdt::user::UserCrdt> {
        let mut crdts = Vec::new();
        for entry in self.users.iter() {
            let user = entry.value().read().await;
            crdts.push(user.to_crdt());
        }
        crdts
    }

    /// Count the number of real (non-service) users.
    ///
    /// This excludes service pseudoclients (NickServ, ChanServ) from the count,
    /// as they are not actual users and should not be reported in LUSERS.
    pub async fn real_user_count(&self) -> usize {
        let mut count = 0;
        for entry in self.users.iter() {
            let user = entry.value().read().await;
            if !user.modes.service {
                count += 1;
            }
        }
        count
    }

    /// Add a local user to the state.
    pub async fn add_local_user(&self, user: User) {
        let uid = user.uid.clone();
        let nick_lower = slirc_proto::irc_to_lower(&user.nick);

        self.nicks.insert(nick_lower, uid.clone());
        self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));

        // Notify observer (local change)
        self.notify_observer(&uid, None).await;
    }

    /// Register a service pseudoclient (NickServ, ChanServ, etc.).
    ///
    /// Unlike `add_local_user`, this is synchronous and does NOT notify
    /// the observer because services are registered at startup before
    /// the server accepts connections.
    pub fn register_service_user(&mut self, user: User) {
        let uid = user.uid.clone();
        let nick_lower = slirc_proto::irc_to_lower(&user.nick);

        self.nicks.insert(nick_lower, uid.clone());
        self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));
    }

    /// Merge a UserCrdt into the local state.
    pub async fn merge_user_crdt(
        &self,
        crdt: slirc_crdt::user::UserCrdt,
        source: Option<ServerId>,
    ) {
        let uid = crdt.uid.clone();
        let incoming_nick = crdt.nick.value().clone();
        let incoming_nick_lower = slirc_proto::irc_to_lower(&incoming_nick);
        let incoming_ts = crdt.nick.timestamp();

        // Check for Nick Collision
        let collision_info = if let Some(existing_uid) = self.nicks.get(&incoming_nick_lower) {
            if *existing_uid != uid {
                // Collision found. Get TS.
                if let Some(user_arc) = self.users.get(&*existing_uid) {
                    let user = user_arc.read().await;
                    Some((existing_uid.clone(), user.last_modified))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some((existing_uid, existing_ts)) = collision_info {
            // We have a collision. Resolve it using TS rules.
            if incoming_ts < existing_ts {
                // Incoming is older (Winner). Kill existing.
                self.kill_user(&existing_uid, "Nick collision (older wins)")
                    .await;
                crate::metrics::DISTRIBUTED_COLLISIONS_TOTAL
                    .with_label_values(&["nick", "kill_existing"])
                    .inc();
                // Proceed to merge incoming
            } else if incoming_ts > existing_ts {
                // Incoming is newer (Loser).
                // Merge then kill so we have the record
                self.perform_merge(crdt, source).await;
                self.kill_user(&uid, "Nick collision (newer loses)").await;
                crate::metrics::DISTRIBUTED_COLLISIONS_TOTAL
                    .with_label_values(&["nick", "kill_incoming"])
                    .inc();

                // Restore the existing user's nick index, because perform_merge overwrote it
                // and kill_user removed it.
                if let Some(existing_user_arc) = self.users.get(&existing_uid) {
                    let existing_user = existing_user_arc.read().await;
                    let nick_lower = slirc_proto::irc_to_lower(&existing_user.nick);
                    self.nicks.insert(nick_lower, existing_uid.clone());
                }
                return;
            } else {
                // Tie. Kill both.
                self.kill_user(&existing_uid, "Nick collision (tie)").await;
                self.perform_merge(crdt, source).await;
                self.kill_user(&uid, "Nick collision (tie)").await;
                crate::metrics::DISTRIBUTED_COLLISIONS_TOTAL
                    .with_label_values(&["nick", "kill_both"])
                    .inc();
                return;
            }
        }

        self.perform_merge(crdt, source).await;
    }

    /// Helper to perform the actual merge logic.
    async fn perform_merge(&self, crdt: slirc_crdt::user::UserCrdt, source: Option<ServerId>) {
        let uid = crdt.uid.clone();

        if let Some(user_arc) = self.users.get(&uid) {
            let mut user = user_arc.value().write().await;
            let old_nick_lower = slirc_proto::irc_to_lower(&user.nick);
            user.merge(crdt);
            let new_nick_lower = slirc_proto::irc_to_lower(&user.nick);

            if old_nick_lower != new_nick_lower {
                self.nicks.remove(&old_nick_lower);
                self.nicks.insert(new_nick_lower, uid.clone());
            }
        } else {
            let user = User::from_crdt(crdt);
            let nick_lower = slirc_proto::irc_to_lower(&user.nick);
            self.nicks.insert(nick_lower, uid.clone());
            self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));
        }

        // Notify observer to propagate the change (Split Horizon handled by source)
        self.notify_observer(&uid, source).await;
    }

    /// Kill a user (remove from state and notify observer).
    pub async fn kill_user(&self, uid: &str, reason: &str) {
        if let Some((_, user_arc)) = self.users.remove(uid) {
            let user = user_arc.read().await;
            let nick_lower = slirc_proto::irc_to_lower(&user.nick);
            self.nicks.remove(&nick_lower);
            self.senders.remove(uid);

            // Record WHOWAS
            self.record_whowas(&user.nick, &user.user, &user.host, &user.realname);

            // Notify observer
            if let Some(observer) = &self.observer {
                observer.on_user_quit(uid, reason, None);
            }
        }
    }

    /// Send a server notice to all operators subscribed to the given snomask.
    ///
    /// # Arguments
    /// - `mask`: The snomask character (e.g., 'c' for connect, 'k' for kill).
    /// - `message`: The message text.
    pub async fn send_snomask(&self, mask: char, message: &str) {
        let notice_msg = Arc::new(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(self.server_name.clone())),
            command: Command::NOTICE(
                "*".to_string(), // Target is * for server notices
                format!("*** Notice -- {}", message),
            ),
        });

        for user_entry in self.users.iter() {
            let user_guard = user_entry.value().read().await;
            if user_guard.modes.has_snomask(mask)
                && let Some(sender) = self.senders.get(&user_guard.uid)
            {
                let _ = sender.send(notice_msg.clone()).await;
            }
        }
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
            server: self.server_name.clone(),
            logout_time: chrono::Utc::now().timestamp_millis(),
        };

        self.whowas
            .entry(nick_lower.clone())
            .or_default()
            .push_front(entry);

        // Prune old entries if over the limit
        if let Some(mut entries) = self.whowas.get_mut(&nick_lower) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_crdt::clock::HybridTimestamp;
    use slirc_crdt::user::UserCrdt;

    fn create_user(uid: &str, nick: &str, ts: HybridTimestamp) -> UserCrdt {
        let mut user = UserCrdt::new(
            uid.to_string(),
            nick.to_string(),
            "user".to_string(),
            "Real Name".to_string(),
            "host".to_string(),
            "host".to_string(),
            ts,
        );
        // Ensure nick register has the correct timestamp
        user.nick.update(nick.to_string(), ts);
        user
    }

    #[tokio::test]
    async fn test_nick_collision_older_wins() {
        let manager = UserManager::new("001".to_string(), "test.server".to_string());
        let sid_a = ServerId::new("00A");
        let sid_b = ServerId::new("00B");

        // User A (TS=100) - Older
        let t1 = HybridTimestamp::new(100, 0, &sid_a);
        let user_a = create_user("00A000001", "Alice", t1);
        manager.merge_user_crdt(user_a, None).await;

        // User B (TS=200) - Newer, should lose
        let t2 = HybridTimestamp::new(200, 0, &sid_b);
        let user_b = create_user("00B000001", "Alice", t2);

        manager.merge_user_crdt(user_b, Some(sid_b)).await;

        // Verify A remains, B is gone
        assert!(
            manager.users.contains_key("00A000001"),
            "User A should remain"
        );
        assert!(
            !manager.users.contains_key("00B000001"),
            "User B should be killed"
        );
        assert_eq!(*manager.nicks.get("alice").unwrap(), "00A000001");
    }

    #[tokio::test]
    async fn test_nick_collision_newer_loses() {
        let manager = UserManager::new("001".to_string(), "test.server".to_string());
        let sid_a = ServerId::new("00A");
        let sid_b = ServerId::new("00B");

        // User A (TS=200) - Newer
        let t1 = HybridTimestamp::new(200, 0, &sid_a);
        let user_a = create_user("00A000001", "Alice", t1);
        manager.merge_user_crdt(user_a, None).await;

        // User B (TS=100) - Older, should win
        let t2 = HybridTimestamp::new(100, 0, &sid_b);
        let user_b = create_user("00B000001", "Alice", t2);

        manager.merge_user_crdt(user_b, Some(sid_b)).await;

        // Verify B remains, A is gone
        assert!(
            !manager.users.contains_key("00A000001"),
            "User A should be killed"
        );
        assert!(
            manager.users.contains_key("00B000001"),
            "User B should remain"
        );
        assert_eq!(*manager.nicks.get("alice").unwrap(), "00B000001");
    }

    #[tokio::test]
    async fn test_nick_collision_tie_kills_both() {
        let manager = UserManager::new("001".to_string(), "test.server".to_string());
        let sid_a = ServerId::new("00A");
        let sid_b = ServerId::new("00B");

        // User A (TS=100)
        let t1 = HybridTimestamp::new(100, 0, &sid_a);
        let user_a = create_user("00A000001", "Alice", t1);
        manager.merge_user_crdt(user_a, None).await;

        // User B (TS=100) - Tie
        // We use the same timestamp (including SID) to force equality
        let t2 = HybridTimestamp::new(100, 0, &sid_a);
        let user_b = create_user("00B000001", "Alice", t2);

        manager.merge_user_crdt(user_b, Some(sid_b)).await;

        // Verify both gone
        assert!(
            !manager.users.contains_key("00A000001"),
            "User A should be killed"
        );
        assert!(
            !manager.users.contains_key("00B000001"),
            "User B should be killed"
        );
        assert!(!manager.nicks.contains_key("alice"));
    }
}
