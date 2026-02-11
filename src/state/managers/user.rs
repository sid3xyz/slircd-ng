//! User management state and behavior.
//!
//! This module contains the `UserManager` struct, which isolates all
//! user-related state and logic from the main Matrix struct.

use crate::state::client::SessionId;
use crate::state::{Uid, UidGenerator, User, WhowasEntry, observer::StateObserver};
use dashmap::DashMap;
use slirc_proto::sync::clock::ServerId;
use slirc_proto::{Command, Message, Prefix};
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;

use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Default maximum number of WHOWAS entries to keep per nickname.
const DEFAULT_WHOWAS_GROUPSIZE: usize = 10;
/// Default maximum unique nicks in WHOWAS history (LRU eviction).
const DEFAULT_WHOWAS_MAXGROUPS: usize = 1000;

/// Manages all user-related state and behavior.
///
/// The UserManager is responsible for:
/// - Tracking connected users and their nicknames.
/// - Managing message senders for broadcasting.
/// - Maintaining WHOWAS history for disconnected users.
/// - Generating unique identifiers (UIDs).
/// - Handling server-wide notices (snomasks).
///   Sender bound to a specific session, used for per-session routing.
#[derive(Clone)]
pub struct SessionSender {
    pub session_id: SessionId,
    pub tx: mpsc::Sender<Arc<Message>>,
}

pub struct UserManager {
    pub users: DashMap<Uid, Arc<RwLock<User>>>,
    pub nicks: DashMap<String, Vec<Uid>>,
    /// Senders for message routing. For bouncer mode, multiple sessions may share a UID,
    /// so each UID can have multiple senders.
    pub senders: DashMap<Uid, Vec<SessionSender>>,
    /// Per-session capabilities (IRCv3 caps negotiated by that session).
    pub session_caps: DashMap<SessionId, HashSet<String>>,
    pub whowas: DashMap<String, VecDeque<WhowasEntry>>,
    pub uid_gen: UidGenerator,
    pub enforce_timers: DashMap<Uid, Instant>,
    /// This server's name (required for snomask and whowas).
    pub server_name: String,
    /// This server's SID (TS6).
    pub server_sid: String,

    pub stats_manager: Option<Arc<crate::state::managers::stats::StatsManager>>,
    /// Observer for state changes (Innovation 2).
    pub observer: Option<Arc<dyn StateObserver>>,

    // WHOWAS limits (Audit Finding #4 DoS protection)
    whowas_maxgroups: usize,
    whowas_groupsize: usize,
    whowas_entry_ttl_days: i64,
    /// LRU order tracker: front = oldest, back = newest
    whowas_lru: Mutex<VecDeque<String>>,

    pub max_local_users: std::sync::atomic::AtomicUsize,
    pub max_global_users: std::sync::atomic::AtomicUsize,
}

impl UserManager {
    pub fn new(server_sid: String, server_name: String) -> Self {
        Self {
            users: DashMap::new(),
            nicks: DashMap::new(),
            senders: DashMap::new(),
            session_caps: DashMap::new(),
            whowas: DashMap::new(),
            uid_gen: UidGenerator::new(server_sid.clone()),
            enforce_timers: DashMap::new(),
            server_name,
            server_sid,

            stats_manager: None,
            observer: None,

            whowas_maxgroups: DEFAULT_WHOWAS_MAXGROUPS,
            whowas_groupsize: DEFAULT_WHOWAS_GROUPSIZE,
            whowas_entry_ttl_days: 7,
            whowas_lru: Mutex::new(VecDeque::new()),

            max_local_users: std::sync::atomic::AtomicUsize::new(0),
            max_global_users: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Get count of real users (excluding services/bots if tagged).
    /// Currently returns total user count until service tagging is fully implemented.
    pub async fn real_user_count(&self) -> usize {
        self.users.len()
    }

    /// Configure WHOWAS limits from config.
    ///
    /// Call this after construction with values from `LimitsConfig`.
    pub fn configure_whowas(&mut self, maxgroups: usize, groupsize: usize, entry_ttl_days: i64) {
        self.whowas_maxgroups = maxgroups;
        self.whowas_groupsize = groupsize;
        self.whowas_entry_ttl_days = entry_ttl_days;
    }

    /// Update the last_active timestamp for a user.
    ///
    /// This should be called whenever the user sends a command.
    /// Uses Relaxed ordering as strict consistency is not required for idle time.
    pub async fn update_last_active(&self, uid: &str) {
        if let Some(user_arc) = self.users.get(uid) {
            user_arc
                .read() // Acquire read lock (AtomicI64 has interior mutability)
                .await
                .last_active
                .store(
                    chrono::Utc::now().timestamp_millis(),
                    std::sync::atomic::Ordering::Relaxed,
                );
        }
    }

    /// Set the stats manager for the user manager.
    pub fn set_stats_manager(&mut self, stats: Arc<crate::state::managers::stats::StatsManager>) {
        self.stats_manager = Some(stats);
    }

    /// Get the first UID for a given nickname (for legacy single-connection lookups).
    ///
    /// For bouncer support, this returns the first UID in the list.
    /// Most code should migrate to handling multiple UIDs per nick.
    pub fn get_first_uid(&self, nick_lower: &str) -> Option<String> {
        self.nicks.get(nick_lower).and_then(|v| v.first().cloned())
    }

    /// Send a message to all sessions for a given UID.
    /// For bouncer mode, multiple sessions may share a UID, so we broadcast to all.
    /// Returns the number of sessions the message was sent to.
    pub async fn send_to_uid(&self, uid: &str, msg: Arc<Message>) -> usize {
        if let Some(senders) = self.senders.get(uid) {
            let senders_clone: Vec<_> = senders.value().clone();
            drop(senders); // Release DashMap lock before awaiting

            let mut sent = 0;
            for sess in senders_clone {
                if sess.tx.send(msg.clone()).await.is_ok() {
                    sent += 1;
                }
            }
            sent
        } else {
            0
        }
    }

    /// Send a message to a specific session of a user.
    /// Returns true if the session was found and the message was sent (or queued).
    pub async fn send_to_session(
        &self,
        uid: &str,
        session_id: SessionId,
        msg: Arc<Message>,
    ) -> bool {
        if let Some(senders) = self.senders.get(uid) {
            let senders_vec = senders.value();
            if let Some(sender) = senders_vec.iter().find(|s| s.session_id == session_id) {
                return sender.tx.send(msg).await.is_ok();
            }
        }
        false
    }

    /// Try to send a message to all sessions for a given UID (non-blocking).
    /// For bouncer mode, multiple sessions may share a UID, so we broadcast to all.
    /// Returns the number of sessions the message was sent to.
    pub fn try_send_to_uid(&self, uid: &str, msg: Arc<Message>) -> usize {
        if let Some(senders) = self.senders.get(uid) {
            let mut sent = 0;
            for sess in senders.value().iter() {
                if sess.tx.try_send(msg.clone()).is_ok() {
                    sent += 1;
                }
            }
            sent
        } else {
            0
        }
    }

    /// Get a cloned list of senders for a UID (for cases that need direct access).
    #[allow(dead_code)]
    pub fn get_senders_cloned(&self, uid: &str) -> Option<Vec<SessionSender>> {
        self.senders.get(uid).map(|r| r.value().clone())
    }

    /// Get a cloned first sender for a UID (for backward compatibility).
    /// This returns the first sender if any exist.
    pub fn get_first_sender(&self, uid: &str) -> Option<mpsc::Sender<Arc<Message>>> {
        self.senders
            .get(uid)
            .and_then(|r| r.value().first().map(|s| s.tx.clone()))
    }

    /// Register a session sender and its initial capabilities under a UID.
    pub fn register_session_sender(
        &self,
        uid: &str,
        session_id: SessionId,
        sender: mpsc::Sender<Arc<Message>>,
        caps: HashSet<String>,
    ) {
        self.session_caps.insert(session_id, caps);
        // Prevent duplicate session registrations; update existing sender if present
        if let Some(mut entry) = self.senders.get_mut(uid) {
            let vec = entry.value_mut();
            if let Some(existing) = vec.iter_mut().find(|s| s.session_id == session_id) {
                existing.tx = sender;
                return;
            }
            vec.push(SessionSender {
                session_id,
                tx: sender,
            });
        } else {
            self.senders.insert(
                uid.to_string(),
                vec![SessionSender {
                    session_id,
                    tx: sender,
                }],
            );
        }
    }

    /// Update capabilities for a specific session.
    /// Update capabilities for a specific session.
    pub fn update_session_caps(&self, session_id: SessionId, caps: HashSet<String>) {
        self.session_caps.insert(session_id, caps);
    }

    /// Get capabilities for a specific session.
    pub fn get_session_caps(&self, session_id: SessionId) -> Option<HashSet<String>> {
        self.session_caps
            .get(&session_id)
            .map(|c| c.value().clone())
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
            // Clone Arc to release DashMap lock before awaiting
            let user_arc = self.users.get(uid).map(|r| r.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                let crdt = user.to_crdt();
                observer.on_user_update(&crdt, source);
            }
        }
    }

    /// Add a local user to the state.
    pub async fn add_local_user(&self, user: User) {
        let uid = user.uid.clone();
        let nick_lower = slirc_proto::irc_to_lower(&user.nick);
        let is_invisible = user.modes.invisible;

        self.nicks
            .entry(nick_lower)
            .or_insert_with(Vec::new)
            .push(uid.clone());
        self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));

        // Update stats
        if let Some(stats) = &self.stats_manager {
            stats.user_connected();
            // If user already has invisible mode (e.g., from default_user_modes),
            // count them as invisible immediately.
            if is_invisible {
                stats.user_set_invisible();
            }
        }

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

        self.nicks
            .entry(nick_lower)
            .or_insert_with(Vec::new)
            .push(uid.clone());
        self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));
    }

    /// Merge a UserCrdt into the local state.
    pub async fn merge_user_crdt(
        &self,
        crdt: slirc_proto::sync::user::UserCrdt,
        source: Option<ServerId>,
    ) {
        let uid = crdt.uid.clone();
        let incoming_nick = crdt.nick.value().clone();
        let incoming_nick_lower = slirc_proto::irc_to_lower(&incoming_nick);
        let incoming_ts = crdt.nick.timestamp();

        // Check for Nick Collision
        // Clone Arc and UID to release DashMap lock before awaiting
        let collision_info = if let Some(existing_uids) = self.nicks.get(&incoming_nick_lower) {
            // For now, take the first UID (will need account-based logic later)
            if let Some(first_uid) = existing_uids.first() {
                if first_uid != &uid {
                    // Collision found. Get TS.
                    let existing_uid_cloned = first_uid.clone();
                    let user_arc = self.users.get(first_uid).map(|r| r.value().clone());
                    drop(existing_uids); // Release nicks lock
                    if let Some(user_arc) = user_arc {
                        let user = user_arc.read().await;
                        Some((existing_uid_cloned, user.last_modified))
                    } else {
                        None
                    }
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
                self.kill_user(&existing_uid, "Nick collision (older wins)", source.clone())
                    .await;
                crate::metrics::inc_distributed_collisions("nick", "kill_existing");
                // Proceed to merge incoming
            } else if incoming_ts > existing_ts {
                // Incoming is newer (Loser).
                // Merge then kill so we have the record
                self.perform_merge(crdt, source.clone()).await;
                self.kill_user(&uid, "Nick collision (newer loses)", source)
                    .await;
                crate::metrics::inc_distributed_collisions("nick", "kill_incoming");

                // Restore the existing user's nick index, because perform_merge may have modified it
                // and kill_user removed the new UID. Ensure existing UID is still present.
                let existing_user_arc = self.users.get(&existing_uid).map(|r| r.value().clone());
                if let Some(existing_user_arc) = existing_user_arc {
                    let existing_user = existing_user_arc.read().await;
                    let nick_lower = slirc_proto::irc_to_lower(&existing_user.nick);
                    // Only add if not already present (defensive)
                    if let Some(mut vec) = self.nicks.get_mut(&nick_lower) {
                        if !vec.contains(&existing_uid) {
                            vec.push(existing_uid.clone());
                        }
                    } else {
                        self.nicks.insert(nick_lower, vec![existing_uid.clone()]);
                    }
                }
                return;
            } else {
                // Tie. Kill both.
                self.kill_user(&existing_uid, "Nick collision (tie)", source.clone())
                    .await;
                self.perform_merge(crdt, source.clone()).await;
                self.kill_user(&uid, "Nick collision (tie)", source).await;
                crate::metrics::inc_distributed_collisions("nick", "kill_both");
                return;
            }
        }

        self.perform_merge(crdt, source).await;
    }

    /// Helper to perform the actual merge logic.
    async fn perform_merge(
        &self,
        crdt: slirc_proto::sync::user::UserCrdt,
        source: Option<ServerId>,
    ) {
        let uid = crdt.uid.clone();

        // Clone Arc to release DashMap lock before awaiting
        let user_arc = self.users.get(&uid).map(|r| r.value().clone());
        if let Some(user_arc) = user_arc {
            let mut user = user_arc.write().await;
            let old_nick_lower = slirc_proto::irc_to_lower(&user.nick);
            let old_oper = user.modes.oper;
            let old_invisible = user.modes.invisible;
            let is_local = user.uid.starts_with(&self.server_sid);

            user.merge_crdt(crdt);

            let new_nick_lower = slirc_proto::irc_to_lower(&user.nick);
            let new_oper = user.modes.oper;
            let new_invisible = user.modes.invisible;

            // Update stats for mode changes
            if let Some(stats) = &self.stats_manager {
                // Oper change
                if old_oper != new_oper {
                    if new_oper {
                        if is_local {
                            stats.user_opered();
                        } else {
                            stats.remote_user_opered();
                        }
                    } else if is_local {
                        stats.user_deopered();
                    } else {
                        stats.remote_user_deopered();
                    }
                }

                // Invisible change (only tracked locally in current StatsManager)
                if is_local && old_invisible != new_invisible {
                    if new_invisible {
                        stats.user_set_invisible();
                    } else {
                        stats.user_unset_invisible();
                    }
                }
            }

            if old_nick_lower != new_nick_lower {
                // Remove this UID from the old nick's vector
                if let Some(mut old_vec) = self.nicks.get_mut(&old_nick_lower) {
                    old_vec.retain(|u| u != &uid);
                    if old_vec.is_empty() {
                        drop(old_vec);
                        self.nicks.remove(&old_nick_lower);
                    }
                }
                // Add to new nick's vector
                self.nicks
                    .entry(new_nick_lower)
                    .or_insert_with(Vec::new)
                    .push(uid.clone());
            }
        } else {
            let user = User::from_crdt(crdt);
            let nick_lower = slirc_proto::irc_to_lower(&user.nick);
            let is_remote = source.is_some();
            let is_oper = user.modes.oper;

            self.nicks
                .entry(nick_lower)
                .or_insert_with(Vec::new)
                .push(uid.clone());
            self.users.insert(uid.clone(), Arc::new(RwLock::new(user)));

            // Update stats for new remote users
            if let Some(stats) = &self.stats_manager
                && is_remote
            {
                stats.remote_user_connected();
                if is_oper {
                    stats.remote_user_opered();
                }
            }
        }

        // Notify observer to propagate the change (Split Horizon handled by source)
        self.notify_observer(&uid, source).await;
    }

    /// Kill a user (remove from state and notify observer).
    ///
    /// Automatically detects local vs. remote users by UID prefix and updates
    /// StatsManager accordingly.
    pub async fn kill_user(&self, uid: &str, reason: &str, _source: Option<ServerId>) {
        if let Some((_, user_arc)) = self.users.remove(uid) {
            let user = user_arc.read().await;
            let nick_lower = slirc_proto::irc_to_lower(&user.nick);
            let is_oper = user.modes.oper;
            let is_invisible = user.modes.invisible;

            // Auto-detect local vs remote by UID prefix (first 3 chars = SID)
            let is_local = uid.starts_with(&self.server_sid);

            // Update stats based on user locality
            if let Some(stats) = &self.stats_manager {
                if is_local {
                    stats.user_disconnected();
                    if is_invisible {
                        stats.user_unset_invisible();
                    }
                    if is_oper {
                        stats.user_deopered();
                    }
                } else {
                    stats.remote_user_disconnected();
                    if is_oper {
                        stats.remote_user_deopered();
                    }
                }
            }

            // Remove this UID from the nick vector
            if let Some(mut nick_vec) = self.nicks.get_mut(&nick_lower) {
                nick_vec.retain(|u| u != uid);
                if nick_vec.is_empty() {
                    drop(nick_vec);
                    self.nicks.remove(&nick_lower);
                }
            }

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

        // Collect user Arc + UID pairs to release DashMap lock before awaiting
        let user_data: Vec<_> = self
            .users
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (uid, user_arc) in user_data {
            let user_guard = user_arc.read().await;
            if user_guard.modes.has_snomask(mask) {
                self.send_to_uid(&uid, notice_msg.clone()).await;
            }
        }
    }

    /// Send a server notice to all IRC operators, regardless of snomask subscriptions.
    pub async fn send_notice_to_opers(&self, message: &str) {
        let notice_msg = Arc::new(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(self.server_name.clone())),
            command: Command::NOTICE("*".to_string(), format!("*** Notice -- {}", message)),
        });

        let user_data: Vec<_> = self
            .users
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (uid, user_arc) in user_data {
            let user_guard = user_arc.read().await;
            if user_guard.modes.oper {
                self.send_to_uid(&uid, notice_msg.clone()).await;
            }
        }
    }

    /// Increment the unregistered connections counter.
    ///
    /// Call this when a new connection is established (before registration).
    pub fn increment_unregistered(&self) {
        if let Some(stats) = &self.stats_manager {
            stats.increment_unregistered();
        }
    }

    /// Decrement the unregistered connections counter.
    ///
    /// Call this when a connection registers or disconnects before registration.
    pub fn decrement_unregistered(&self) {
        if let Some(stats) = &self.stats_manager {
            stats.decrement_unregistered();
        }
    }

    /// Record a WHOWAS entry for a user who is disconnecting.
    ///
    /// Entries are stored per-nick (lowercase) with most recent first.
    /// Implements LRU eviction when maxgroups is exceeded to prevent DoS.
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

        // Check if this is a new nick (not already in whowas)
        let is_new_nick = !self.whowas.contains_key(&nick_lower);

        // LRU eviction: if adding a new nick and we're at capacity, evict oldest
        if is_new_nick {
            if let Ok(mut lru) = self.whowas_lru.lock() {
                // If at capacity, evict oldest nick
                while lru.len() >= self.whowas_maxgroups {
                    if let Some(oldest) = lru.pop_front() {
                        self.whowas.remove(&oldest);
                    } else {
                        break;
                    }
                }
                // Add new nick to LRU (back = newest)
                lru.push_back(nick_lower.clone());
            }
        } else {
            // Update LRU position for existing nick (move to back)
            if let Ok(mut lru) = self.whowas_lru.lock() {
                lru.retain(|n| n != &nick_lower);
                lru.push_back(nick_lower.clone());
            }
        }

        // Insert entry
        self.whowas
            .entry(nick_lower.clone())
            .or_default()
            .push_front(entry);

        // Prune per-nick entries if over groupsize limit
        if let Some(mut entries) = self.whowas.get_mut(&nick_lower) {
            while entries.len() > self.whowas_groupsize {
                entries.pop_back();
            }
        }
    }

    /// Clean up expired WHOWAS entries.
    ///
    /// Removes entries older than `whowas_entry_ttl_days`. Called hourly by
    /// the lifecycle maintenance task to prevent unbounded growth.
    pub fn cleanup_whowas(&self) {
        let cutoff = chrono::Utc::now().timestamp_millis()
            - (self.whowas_entry_ttl_days * 24 * 3600 * 1000);

        // Also clean up LRU tracker for removed nicks
        let removed_nicks: Vec<String> = self
            .whowas
            .iter()
            .filter_map(|entry| {
                let all_expired = entry.value().iter().all(|e| e.logout_time <= cutoff);
                if all_expired {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        self.whowas.retain(|_, entries| {
            entries.retain(|e| e.logout_time > cutoff);
            !entries.is_empty()
        });

        if !removed_nicks.is_empty()
            && let Ok(mut lru) = self.whowas_lru.lock()
        {
            lru.retain(|n| !removed_nicks.contains(n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::sync::clock::HybridTimestamp;
    use slirc_proto::sync::user::UserCrdt;

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
        let alice_vec = manager.nicks.get("alice").unwrap();
        assert_eq!(alice_vec.len(), 1);
        assert_eq!(alice_vec[0], "00A000001");
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
        let alice_vec = manager.nicks.get("alice").unwrap();
        assert_eq!(alice_vec.len(), 1);
        assert_eq!(alice_vec[0], "00B000001");
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

    #[tokio::test]
    async fn test_kill_user_updates_stats() {
        use crate::state::managers::stats::StatsManager;

        let stats = Arc::new(StatsManager::new());
        let mut manager = UserManager::new("001".to_string(), "test.server".to_string());
        manager.set_stats_manager(stats.clone());

        // Simulate initial state with 1 local user already connected
        stats.user_connected(); // Local user
        assert_eq!(stats.local_users(), 1);
        assert_eq!(stats.global_users(), 1);

        // Create and kill local user (UID starts with "001")
        let sid_local = ServerId::new("001");
        let t1 = HybridTimestamp::new(100, 0, &sid_local);
        let local_user = create_user("001AAAA01", "LocalUser", t1);
        manager.merge_user_crdt(local_user, None).await;

        manager
            .kill_user("001AAAA01", "Test disconnect", None)
            .await;
        assert_eq!(stats.local_users(), 0, "Local user count should decrease");
        assert_eq!(stats.global_users(), 0, "Global user count should decrease");

        // Create and kill remote user (UID starts with "00A")
        // Note: merge_user_crdt for remote users increments stats internally
        let sid_remote = ServerId::new("00A");
        let t2 = HybridTimestamp::new(100, 0, &sid_remote);
        let remote_user = create_user("00AAAAA01", "RemoteUser", t2);
        manager
            .merge_user_crdt(remote_user, Some(sid_remote.clone()))
            .await;

        // After merge, global should be 1 (remote user)
        assert_eq!(stats.global_users(), 1, "Remote user added via merge");

        manager
            .kill_user("00AAAAA01", "Netsplit", Some(sid_remote))
            .await;
        assert_eq!(stats.local_users(), 0, "Local count should remain 0");
        assert_eq!(
            stats.global_users(),
            0,
            "Global user count should be 0 after remote kill"
        );
    }
}
