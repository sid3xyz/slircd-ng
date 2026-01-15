//! CRDT wrapper for Channel state.
//!
//! This module provides `ChannelCrdt`, a CRDT-enabled wrapper around channel
//! state that supports distributed synchronization across linked servers.

use super::clock::HybridTimestamp;
use super::traits::{AwSet, Crdt, LwwRegister};
use std::collections::HashMap;

/// CRDT-enabled channel state for distributed synchronization.
///
/// Uses different CRDT strategies for different fields:
/// - **LWW (Last-Writer-Wins)**: topic, key, limit, modes
/// - **`AWSet` (Add-Wins Set)**: members, bans, invites, excepts
///
/// Channel membership uses a specialized `MembershipCrdt` that tracks
/// both presence and per-member modes (op, voice, etc.).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelCrdt {
    /// Channel name (normalized to lowercase).
    pub name: String,

    /// Channel topic.
    pub topic: LwwRegister<Option<TopicCrdt>>,

    /// Channel modes (each mode is independent).
    pub modes: ChannelModesCrdt,

    /// Channel key (+k password).
    pub key: LwwRegister<Option<String>>,

    /// User limit (+l).
    pub limit: LwwRegister<Option<u32>>,

    /// Channel members with their modes.
    pub members: MembershipCrdt,

    /// Ban list (+b).
    pub bans: AwSet<ListEntryCrdt>,

    /// Invite exceptions (+I).
    pub invites: AwSet<ListEntryCrdt>,

    /// Ban exceptions (+e).
    pub excepts: AwSet<ListEntryCrdt>,

    /// Timestamp when channel was created.
    pub created_at: HybridTimestamp,
}

/// CRDT-enabled topic with setter and timestamp.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct TopicCrdt {
    /// The topic text.
    pub text: String,
    /// Who set the topic.
    pub set_by: String,
    /// Unix timestamp when topic was set.
    pub set_at: i64,
}

/// A list entry (ban, invite, except) as a CRDT-compatible type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct ListEntryCrdt {
    /// The ban/invite/except mask (e.g., *!*@host).
    pub mask: String,
    /// Who set the entry.
    pub set_by: String,
    /// Unix timestamp when entry was set.
    pub set_at: i64,
}

/// CRDT-enabled channel modes.
///
/// Each boolean mode is an independent LWW register.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelModesCrdt {
    /// +n: No external messages.
    pub no_external: LwwRegister<bool>,
    /// +t: Only ops can set topic.
    pub topic_ops_only: LwwRegister<bool>,
    /// +m: Moderated (only voiced users can speak).
    pub moderated: LwwRegister<bool>,
    /// +i: Invite-only.
    pub invite_only: LwwRegister<bool>,
    /// +s: Secret (not shown in LIST).
    pub secret: LwwRegister<bool>,
    /// +p: Private (not shown in WHOIS channel list).
    pub private: LwwRegister<bool>,
    /// +r: Registered users only.
    pub registered_only: LwwRegister<bool>,
    /// +c: No colors allowed.
    pub no_colors: LwwRegister<bool>,
    /// +C: No CTCP allowed.
    pub no_ctcp: LwwRegister<bool>,
    /// +S: SSL/TLS users only.
    pub ssl_only: LwwRegister<bool>,
    /// +z: Reduced moderation (ops/voiced see blocked messages).
    pub reduced_moderation: LwwRegister<bool>,
}

impl ChannelModesCrdt {
    /// Create default channel modes.
    #[must_use]
    pub fn new(timestamp: HybridTimestamp) -> Self {
        Self {
            no_external: LwwRegister::new(true, timestamp), // +n is common default
            topic_ops_only: LwwRegister::new(true, timestamp), // +t is common default
            moderated: LwwRegister::new(false, timestamp),
            invite_only: LwwRegister::new(false, timestamp),
            secret: LwwRegister::new(false, timestamp),
            private: LwwRegister::new(false, timestamp),
            registered_only: LwwRegister::new(false, timestamp),
            no_colors: LwwRegister::new(false, timestamp),
            no_ctcp: LwwRegister::new(false, timestamp),
            ssl_only: LwwRegister::new(false, timestamp),
            reduced_moderation: LwwRegister::new(false, timestamp),
        }
    }
}

impl Crdt for ChannelModesCrdt {
    fn merge(&mut self, other: &Self) {
        self.no_external.merge(&other.no_external);
        self.topic_ops_only.merge(&other.topic_ops_only);
        self.moderated.merge(&other.moderated);
        self.invite_only.merge(&other.invite_only);
        self.secret.merge(&other.secret);
        self.private.merge(&other.private);
        self.registered_only.merge(&other.registered_only);
        self.no_colors.merge(&other.no_colors);
        self.no_ctcp.merge(&other.no_ctcp);
        self.ssl_only.merge(&other.ssl_only);
        self.reduced_moderation.merge(&other.reduced_moderation);
    }

    fn dominates(&self, other: &Self) -> bool {
        self.no_external.dominates(&other.no_external)
            && self.topic_ops_only.dominates(&other.topic_ops_only)
            && self.moderated.dominates(&other.moderated)
            && self.invite_only.dominates(&other.invite_only)
            && self.secret.dominates(&other.secret)
            && self.private.dominates(&other.private)
            && self.registered_only.dominates(&other.registered_only)
            && self.no_colors.dominates(&other.no_colors)
            && self.no_ctcp.dominates(&other.no_ctcp)
            && self.ssl_only.dominates(&other.ssl_only)
            && self.reduced_moderation.dominates(&other.reduced_moderation)
    }
}

/// CRDT for channel membership with per-member modes.
///
/// Each member's presence and modes are tracked independently.
/// Uses `AWSet` semantics for presence (JOIN adds, PART/KICK removes).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MembershipCrdt {
    /// Map from UID to member state.
    /// Presence is tracked via `AWSet`, modes via LWW.
    presence: AwSet<String>,
    /// Per-member modes.
    modes: HashMap<String, MemberModesCrdt>,
}

/// Per-member modes (op, voice, etc.) as CRDT.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemberModesCrdt {
    /// Channel owner mode (+q).
    pub owner: LwwRegister<bool>,
    /// Channel admin mode (+a).
    pub admin: LwwRegister<bool>,
    /// Channel operator mode (+o).
    pub op: LwwRegister<bool>,
    /// Half-operator mode (+h).
    pub halfop: LwwRegister<bool>,
    /// Voice mode (+v).
    pub voice: LwwRegister<bool>,
    /// Unix timestamp when user joined channel.
    pub join_time: i64,
}

impl MemberModesCrdt {
    /// Create default member modes (no privileges).
    #[must_use]
    pub fn new(join_time: i64, timestamp: HybridTimestamp) -> Self {
        Self {
            owner: LwwRegister::new(false, timestamp),
            admin: LwwRegister::new(false, timestamp),
            op: LwwRegister::new(false, timestamp),
            halfop: LwwRegister::new(false, timestamp),
            voice: LwwRegister::new(false, timestamp),
            join_time,
        }
    }
}

impl Crdt for MemberModesCrdt {
    fn merge(&mut self, other: &Self) {
        self.owner.merge(&other.owner);
        self.admin.merge(&other.admin);
        self.op.merge(&other.op);
        self.halfop.merge(&other.halfop);
        self.voice.merge(&other.voice);
        // Join time: take earlier time
        if other.join_time < self.join_time {
            self.join_time = other.join_time;
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        self.owner.dominates(&other.owner)
            && self.admin.dominates(&other.admin)
            && self.op.dominates(&other.op)
            && self.halfop.dominates(&other.halfop)
            && self.voice.dominates(&other.voice)
    }
}

impl MembershipCrdt {
    /// Create an empty membership.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a member to the channel.
    pub fn join(&mut self, uid: String, timestamp: HybridTimestamp) {
        self.presence.add(uid.clone(), timestamp);
        let join_time = chrono::Utc::now().timestamp();
        self.modes
            .entry(uid)
            .or_insert_with(|| MemberModesCrdt::new(join_time, timestamp));
    }

    /// Remove a member from the channel.
    pub fn part(&mut self, uid: &str, timestamp: HybridTimestamp) {
        self.presence.remove(&uid.to_string(), timestamp);
        // Keep modes in case they rejoin (for mode persistence across netjoins)
    }

    /// Check if a user is a member.
    #[must_use]
    pub fn contains(&self, uid: &str) -> bool {
        self.presence.contains(&uid.to_string())
    }

    /// Get a member's modes.
    #[must_use]
    pub fn get_modes(&self, uid: &str) -> Option<&MemberModesCrdt> {
        if self.presence.contains(&uid.to_string()) {
            self.modes.get(uid)
        } else {
            None
        }
    }

    /// Get mutable member modes.
    pub fn get_modes_mut(&mut self, uid: &str) -> Option<&mut MemberModesCrdt> {
        if self.presence.contains(&uid.to_string()) {
            self.modes.get_mut(uid)
        } else {
            None
        }
    }

    /// Iterate over present members.
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.presence.iter()
    }

    /// Get the number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.presence.len()
    }

    /// Check if the channel is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.presence.is_empty()
    }
}

impl Crdt for MembershipCrdt {
    fn merge(&mut self, other: &Self) {
        self.presence.merge(&other.presence);

        // Merge modes for all members
        for (uid, other_modes) in &other.modes {
            match self.modes.get_mut(uid) {
                Some(self_modes) => self_modes.merge(other_modes),
                None => {
                    self.modes.insert(uid.clone(), other_modes.clone());
                }
            }
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        if !self.presence.dominates(&other.presence) {
            return false;
        }
        for (uid, other_modes) in &other.modes {
            match self.modes.get(uid) {
                Some(self_modes) if self_modes.dominates(other_modes) => {}
                _ => return false,
            }
        }
        true
    }
}

impl ChannelCrdt {
    /// Create a new channel.
    #[must_use]
    pub fn new(name: String, timestamp: HybridTimestamp) -> Self {
        Self {
            name,
            topic: LwwRegister::new(None, timestamp),
            modes: ChannelModesCrdt::new(timestamp),
            key: LwwRegister::new(None, timestamp),
            limit: LwwRegister::new(None, timestamp),
            members: MembershipCrdt::new(),
            bans: AwSet::new(),
            invites: AwSet::new(),
            excepts: AwSet::new(),
            created_at: timestamp,
        }
    }

    /// Set the channel topic.
    pub fn set_topic(&mut self, text: String, set_by: String, timestamp: HybridTimestamp) {
        let topic = TopicCrdt {
            text,
            set_by,
            set_at: chrono::Utc::now().timestamp(),
        };
        self.topic.update(Some(topic), timestamp);
    }

    /// Clear the channel topic.
    pub fn clear_topic(&mut self, timestamp: HybridTimestamp) {
        self.topic.update(None, timestamp);
    }

    /// Add a user to the channel.
    pub fn join(&mut self, uid: String, timestamp: HybridTimestamp) {
        self.members.join(uid, timestamp);
    }

    /// Remove a user from the channel.
    pub fn part(&mut self, uid: &str, timestamp: HybridTimestamp) {
        self.members.part(uid, timestamp);
    }

    /// Add a ban.
    pub fn add_ban(&mut self, mask: String, set_by: String, timestamp: HybridTimestamp) {
        let entry = ListEntryCrdt {
            mask,
            set_by,
            set_at: chrono::Utc::now().timestamp(),
        };
        self.bans.add(entry, timestamp);
    }

    /// Remove a ban.
    pub fn remove_ban(&mut self, mask: &str, timestamp: HybridTimestamp) {
        // Find and remove the ban entry with matching mask
        // Note: This is O(n) but ban lists are typically small
        let to_remove: Vec<_> = self
            .bans
            .iter()
            .filter(|e| e.mask == mask)
            .cloned()
            .collect();
        for entry in to_remove {
            self.bans.remove(&entry, timestamp);
        }
    }
}

impl Crdt for ChannelCrdt {
    fn merge(&mut self, other: &Self) {
        debug_assert_eq!(self.name, other.name);

        self.topic.merge(&other.topic);
        self.modes.merge(&other.modes);
        self.key.merge(&other.key);
        self.limit.merge(&other.limit);
        self.members.merge(&other.members);
        self.bans.merge(&other.bans);
        self.invites.merge(&other.invites);
        self.excepts.merge(&other.excepts);
        // created_at: take earlier timestamp
        if other.created_at < self.created_at {
            self.created_at = other.created_at;
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        self.topic.dominates(&other.topic)
            && self.modes.dominates(&other.modes)
            && self.key.dominates(&other.key)
            && self.limit.dominates(&other.limit)
            && self.members.dominates(&other.members)
            && self.bans.dominates(&other.bans)
            && self.invites.dominates(&other.invites)
            && self.excepts.dominates(&other.excepts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ServerId;

    fn make_channel(name: &str, server: &ServerId, millis: i64) -> ChannelCrdt {
        let ts = HybridTimestamp::new(millis, 0, server);
        ChannelCrdt::new(name.to_string(), ts)
    }

    #[test]
    fn test_channel_crdt_new() {
        let server = ServerId::new("001");
        let chan = make_channel("#test", &server, 100);

        assert_eq!(chan.name, "#test");
        assert!(chan.topic.value().is_none());
        assert!(chan.key.value().is_none());
        assert!(chan.limit.value().is_none());
        assert!(chan.members.is_empty());
        assert!(chan.bans.is_empty());
    }

    #[test]
    fn test_channel_crdt_default_modes() {
        let server = ServerId::new("001");
        let chan = make_channel("#test", &server, 100);

        // Default modes: +nt
        assert!(*chan.modes.no_external.value());
        assert!(*chan.modes.topic_ops_only.value());
        assert!(!*chan.modes.moderated.value());
        assert!(!*chan.modes.invite_only.value());
    }

    #[test]
    fn test_channel_crdt_concurrent_join() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts_create = HybridTimestamp::new(100, 0, &server1);
        let ts1 = HybridTimestamp::new(200, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server2);

        let mut chan1 = ChannelCrdt::new("#test".to_string(), ts_create);
        chan1.join("001AAA".to_string(), ts1);

        let mut chan2 = chan1.clone();
        chan2.join("002BBB".to_string(), ts2);

        chan1.merge(&chan2);

        assert!(chan1.members.contains("001AAA"));
        assert!(chan1.members.contains("002BBB"));
    }

    #[test]
    fn test_channel_crdt_join_part() {
        let server = ServerId::new("001");
        let mut chan = make_channel("#test", &server, 100);

        let ts_join = HybridTimestamp::new(200, 0, &server);
        chan.join("001AAA".to_string(), ts_join);
        assert!(chan.members.contains("001AAA"));
        assert_eq!(chan.members.len(), 1);

        let ts_part = HybridTimestamp::new(300, 0, &server);
        chan.part("001AAA", ts_part);
        assert!(!chan.members.contains("001AAA"));
        assert!(chan.members.is_empty());
    }

    #[test]
    fn test_channel_crdt_topic_set() {
        let server = ServerId::new("001");
        let mut chan = make_channel("#test", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        chan.set_topic("Hello World".to_string(), "user".to_string(), ts);

        let topic = chan.topic.value().as_ref().unwrap();
        assert_eq!(topic.text, "Hello World");
        assert_eq!(topic.set_by, "user");
    }

    #[test]
    fn test_channel_crdt_topic_clear() {
        let server = ServerId::new("001");
        let mut chan = make_channel("#test", &server, 100);

        let ts1 = HybridTimestamp::new(200, 0, &server);
        chan.set_topic("Topic".to_string(), "user".to_string(), ts1);

        let ts2 = HybridTimestamp::new(300, 0, &server);
        chan.clear_topic(ts2);

        assert!(chan.topic.value().is_none());
    }

    #[test]
    fn test_channel_crdt_topic_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts_create = HybridTimestamp::new(100, 0, &server1);
        let ts1 = HybridTimestamp::new(200, 0, &server1);
        let ts2 = HybridTimestamp::new(300, 0, &server2);

        let mut chan1 = ChannelCrdt::new("#test".to_string(), ts_create);
        chan1.set_topic("Old topic".to_string(), "user1".to_string(), ts1);

        let mut chan2 = chan1.clone();
        chan2.set_topic("New topic".to_string(), "user2".to_string(), ts2);

        chan1.merge(&chan2);

        let topic = chan1.topic.value().as_ref().unwrap();
        assert_eq!(topic.text, "New topic");
        assert_eq!(topic.set_by, "user2");
    }

    #[test]
    fn test_channel_crdt_ban_add_remove() {
        let server = ServerId::new("001");
        let mut chan = make_channel("#test", &server, 100);

        let ts1 = HybridTimestamp::new(200, 0, &server);
        chan.add_ban("*!*@bad.host".to_string(), "oper".to_string(), ts1);

        assert_eq!(chan.bans.len(), 1);
        let ban = chan.bans.iter().next().unwrap();
        assert_eq!(ban.mask, "*!*@bad.host");

        let ts2 = HybridTimestamp::new(300, 0, &server);
        chan.remove_ban("*!*@bad.host", ts2);

        assert!(chan.bans.is_empty());
    }

    #[test]
    fn test_channel_crdt_mode_change() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts_create = HybridTimestamp::new(100, 0, &server1);
        let ts1 = HybridTimestamp::new(200, 0, &server1);
        let ts2 = HybridTimestamp::new(300, 0, &server2);
        let ts3 = HybridTimestamp::new(400, 0, &server2);

        let mut chan1 = ChannelCrdt::new("#test".to_string(), ts_create);
        chan1.join("001AAA".to_string(), ts1);

        // Grant op on server1 at ts2
        if let Some(modes) = chan1.members.get_modes_mut("001AAA") {
            modes.op.update(true, ts2);
        }

        let mut chan2 = chan1.clone();

        // Grant voice on server2 at ts3 (later)
        if let Some(modes) = chan2.members.get_modes_mut("001AAA") {
            modes.voice.update(true, ts3);
        }

        chan1.merge(&chan2);

        let modes = chan1.members.get_modes("001AAA").unwrap();
        assert!(*modes.op.value());
        assert!(*modes.voice.value());
    }

    #[test]
    fn test_channel_crdt_key_and_limit() {
        let server = ServerId::new("001");
        let mut chan = make_channel("#test", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        chan.key.update(Some("secret".to_string()), ts);
        chan.limit.update(Some(50), ts);

        assert_eq!(chan.key.value(), &Some("secret".to_string()));
        assert_eq!(chan.limit.value(), &Some(50));
    }

    #[test]
    fn test_channel_modes_crdt_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server2);

        let mut modes1 = ChannelModesCrdt::new(ts1);
        modes1.moderated.update(true, ts2);

        let mut modes2 = ChannelModesCrdt::new(ts1);
        modes2.secret.update(true, ts2);

        modes1.merge(&modes2);

        assert!(*modes1.moderated.value());
        assert!(*modes1.secret.value());
    }

    #[test]
    fn test_channel_modes_crdt_dominates() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let modes1 = ChannelModesCrdt::new(ts1);
        let modes2 = ChannelModesCrdt::new(ts2);

        assert!(modes2.dominates(&modes1));
        assert!(!modes1.dominates(&modes2));
    }

    #[test]
    fn test_membership_crdt_iter() {
        let server = ServerId::new("001");
        let mut membership = MembershipCrdt::new();

        let ts = HybridTimestamp::new(100, 0, &server);
        membership.join("001AAA".to_string(), ts);
        membership.join("002BBB".to_string(), ts);

        let members: Vec<_> = membership.iter().collect();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&&"001AAA".to_string()));
        assert!(members.contains(&&"002BBB".to_string()));
    }

    #[test]
    fn test_membership_crdt_get_modes_nonmember() {
        let membership = MembershipCrdt::new();

        assert!(membership.get_modes("nonexistent").is_none());
    }

    #[test]
    fn test_member_modes_crdt_join_time_merge() {
        let server = ServerId::new("001");

        let ts = HybridTimestamp::new(100, 0, &server);

        let mut modes1 = MemberModesCrdt::new(200, ts);
        let modes2 = MemberModesCrdt::new(100, ts);

        modes1.merge(&modes2);

        // Earlier join time wins
        assert_eq!(modes1.join_time, 100);
    }

    #[test]
    fn test_channel_crdt_created_at_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(200, 0, &server1);
        let ts2 = HybridTimestamp::new(100, 0, &server2); // Earlier

        let mut chan1 = ChannelCrdt::new("#test".to_string(), ts1);
        let chan2 = ChannelCrdt::new("#test".to_string(), ts2);

        chan1.merge(&chan2);

        // Earlier created_at wins
        assert_eq!(chan1.created_at, ts2);
    }

    #[test]
    fn test_channel_crdt_dominates() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let chan1 = ChannelCrdt::new("#test".to_string(), ts1);

        let mut chan2 = chan1.clone();
        chan2.set_topic("New topic".to_string(), "user".to_string(), ts2);

        // chan1 doesn't dominate chan2 (chan2 has newer topic)
        assert!(!chan1.dominates(&chan2));
        // chan2 dominates chan1
        assert!(chan2.dominates(&chan1));
    }

    #[test]
    fn test_list_entry_crdt_equality() {
        let entry1 = ListEntryCrdt {
            mask: "*!*@host".to_string(),
            set_by: "user".to_string(),
            set_at: 100,
        };
        let entry2 = ListEntryCrdt {
            mask: "*!*@host".to_string(),
            set_by: "user".to_string(),
            set_at: 100,
        };
        let entry3 = ListEntryCrdt {
            mask: "*!*@other".to_string(),
            set_by: "user".to_string(),
            set_at: 100,
        };

        assert_eq!(entry1, entry2);
        assert_ne!(entry1, entry3);
    }

    #[test]
    fn test_topic_crdt_equality() {
        let topic1 = TopicCrdt {
            text: "Hello".to_string(),
            set_by: "user".to_string(),
            set_at: 100,
        };
        let topic2 = TopicCrdt {
            text: "Hello".to_string(),
            set_by: "user".to_string(),
            set_at: 100,
        };

        assert_eq!(topic1, topic2);
    }
}
