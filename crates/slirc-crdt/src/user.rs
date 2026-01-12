//! CRDT wrapper for User state.
//!
//! This module provides `UserCrdt`, a CRDT-enabled wrapper around user state
//! that supports distributed synchronization across linked servers.

use crate::clock::HybridTimestamp;
use crate::traits::{AwSet, Crdt, LwwRegister};
use std::collections::HashSet;

/// CRDT-enabled user state for distributed synchronization.
///
/// Uses different CRDT strategies for different fields:
/// - **LWW (Last-Writer-Wins)**: nick, user, realname, host, away, account
/// - **`AWSet` (Add-Wins Set)**: channels, caps, `silence_list`, snomasks
///
/// This allows concurrent modifications to resolve deterministically.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCrdt {
    /// Unique identifier (never changes after creation).
    pub uid: String,

    /// Nickname (LWW: most recent NICK command wins).
    pub nick: LwwRegister<String>,

    /// Username (LWW: set at registration).
    pub user: LwwRegister<String>,

    /// Real name (LWW: set at registration).
    pub realname: LwwRegister<String>,

    /// Hostname (LWW: may change with CHGHOST).
    pub host: LwwRegister<String>,

    /// Visible (cloaked) hostname (LWW: tracks host changes).
    pub visible_host: LwwRegister<String>,

    /// Account name if identified (LWW: changes on IDENTIFY/LOGOUT).
    pub account: LwwRegister<Option<String>>,

    /// Away message (LWW: changes on AWAY command).
    pub away: LwwRegister<Option<String>>,

    /// Channels the user is in (`AWSet`: JOIN adds, PART/KICK removes).
    pub channels: AwSet<String>,

    /// `IRCv3` capabilities (`AWSet`: enabled during CAP negotiation).
    pub caps: AwSet<String>,

    /// User modes (individual LWW registers for each mode).
    pub modes: UserModesCrdt,

    /// Silence list (`AWSet`: SILENCE +mask adds, SILENCE -mask removes).
    pub silence_list: AwSet<String>,

    /// Accept list (`AWSet`: ACCEPT +nick adds, ACCEPT -nick removes).
    pub accept_list: AwSet<String>,
}

/// CRDT-enabled user modes.
///
/// Each mode flag is an independent LWW register.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserModesCrdt {
    /// Invisible mode (+i).
    pub invisible: LwwRegister<bool>,
    /// Wallops mode (+w).
    pub wallops: LwwRegister<bool>,
    /// IRC operator mode (+o).
    pub oper: LwwRegister<bool>,
    /// Registered with `NickServ` (+r).
    pub registered: LwwRegister<bool>,
    /// Connected via TLS (+z).
    pub secure: LwwRegister<bool>,
    /// Only receive from registered users (+R).
    pub registered_only: LwwRegister<bool>,
    /// Block CTCP (+T).
    pub no_ctcp: LwwRegister<bool>,
    /// Bot flag (+B).
    pub bot: LwwRegister<bool>,
    /// Server notice masks (+s).
    pub snomasks: AwSet<char>,
    /// Operator type name (if opered).
    pub oper_type: LwwRegister<Option<String>>,
}

impl UserModesCrdt {
    /// Create default user modes.
    #[must_use]
    pub fn new(timestamp: HybridTimestamp) -> Self {
        Self {
            invisible: LwwRegister::new(false, timestamp),
            wallops: LwwRegister::new(false, timestamp),
            oper: LwwRegister::new(false, timestamp),
            registered: LwwRegister::new(false, timestamp),
            secure: LwwRegister::new(false, timestamp),
            registered_only: LwwRegister::new(false, timestamp),
            no_ctcp: LwwRegister::new(false, timestamp),
            bot: LwwRegister::new(false, timestamp),
            snomasks: AwSet::new(),
            oper_type: LwwRegister::new(None, timestamp),
        }
    }
}

impl Crdt for UserModesCrdt {
    fn merge(&mut self, other: &Self) {
        self.invisible.merge(&other.invisible);
        self.wallops.merge(&other.wallops);
        self.oper.merge(&other.oper);
        self.registered.merge(&other.registered);
        self.secure.merge(&other.secure);
        self.registered_only.merge(&other.registered_only);
        self.no_ctcp.merge(&other.no_ctcp);
        self.bot.merge(&other.bot);
        self.snomasks.merge(&other.snomasks);
        self.oper_type.merge(&other.oper_type);
    }

    fn dominates(&self, other: &Self) -> bool {
        self.invisible.dominates(&other.invisible)
            && self.wallops.dominates(&other.wallops)
            && self.oper.dominates(&other.oper)
            && self.registered.dominates(&other.registered)
            && self.secure.dominates(&other.secure)
            && self.registered_only.dominates(&other.registered_only)
            && self.no_ctcp.dominates(&other.no_ctcp)
            && self.bot.dominates(&other.bot)
            && self.snomasks.dominates(&other.snomasks)
            && self.oper_type.dominates(&other.oper_type)
    }
}

impl UserCrdt {
    /// Create a new `UserCrdt` from initial values.
    #[must_use]
    pub fn new(
        uid: String,
        nick: String,
        user: String,
        realname: String,
        host: String,
        visible_host: String,
        timestamp: HybridTimestamp,
    ) -> Self {
        Self {
            uid,
            nick: LwwRegister::new(nick, timestamp),
            user: LwwRegister::new(user, timestamp),
            realname: LwwRegister::new(realname, timestamp),
            host: LwwRegister::new(host, timestamp),
            visible_host: LwwRegister::new(visible_host, timestamp),
            account: LwwRegister::new(None, timestamp),
            away: LwwRegister::new(None, timestamp),
            channels: AwSet::new(),
            caps: AwSet::new(),
            modes: UserModesCrdt::new(timestamp),
            silence_list: AwSet::new(),
            accept_list: AwSet::new(),
        }
    }

    /// Update the nickname.
    pub fn set_nick(&mut self, nick: String, timestamp: HybridTimestamp) {
        self.nick.update(nick, timestamp);
    }

    /// Join a channel.
    pub fn join_channel(&mut self, channel: String, timestamp: HybridTimestamp) {
        self.channels.add(channel, timestamp);
    }

    /// Part a channel.
    pub fn part_channel(&mut self, channel: &str, timestamp: HybridTimestamp) {
        self.channels.remove(&channel.to_string(), timestamp);
    }

    /// Set away message.
    pub fn set_away(&mut self, message: Option<String>, timestamp: HybridTimestamp) {
        self.away.update(message, timestamp);
    }

    /// Identify to an account.
    pub fn identify(&mut self, account: String, timestamp: HybridTimestamp) {
        self.account.update(Some(account), timestamp);
        self.modes.registered.update(true, timestamp);
    }

    /// Convert to a set of channels (for compatibility with existing code).
    #[must_use]
    pub fn channels_set(&self) -> HashSet<String> {
        self.channels.iter().cloned().collect()
    }

    /// Convert to a set of capabilities (for compatibility with existing code).
    #[must_use]
    pub fn caps_set(&self) -> HashSet<String> {
        self.caps.iter().cloned().collect()
    }
}

impl Crdt for UserCrdt {
    fn merge(&mut self, other: &Self) {
        // UID must match (or we're merging unrelated users)
        debug_assert_eq!(self.uid, other.uid);

        self.nick.merge(&other.nick);
        self.user.merge(&other.user);
        self.realname.merge(&other.realname);
        self.host.merge(&other.host);
        self.visible_host.merge(&other.visible_host);
        self.account.merge(&other.account);
        self.away.merge(&other.away);
        self.channels.merge(&other.channels);
        self.caps.merge(&other.caps);
        self.modes.merge(&other.modes);
        self.silence_list.merge(&other.silence_list);
        self.accept_list.merge(&other.accept_list);
    }

    fn dominates(&self, other: &Self) -> bool {
        self.nick.dominates(&other.nick)
            && self.user.dominates(&other.user)
            && self.realname.dominates(&other.realname)
            && self.host.dominates(&other.host)
            && self.visible_host.dominates(&other.visible_host)
            && self.account.dominates(&other.account)
            && self.away.dominates(&other.away)
            && self.channels.dominates(&other.channels)
            && self.caps.dominates(&other.caps)
            && self.modes.dominates(&other.modes)
            && self.silence_list.dominates(&other.silence_list)
            && self.accept_list.dominates(&other.accept_list)
    }
}

/// A delta representing changes to a `UserCrdt`.
///
/// Only contains fields that have changed, for efficient network transfer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDelta {
    /// User ID (required to identify the user).
    pub uid: String,
    /// Updated nick (if changed).
    pub nick: Option<LwwRegister<String>>,
    /// Updated away message (if changed).
    pub away: Option<LwwRegister<Option<String>>>,
    /// Updated account (if changed).
    pub account: Option<LwwRegister<Option<String>>>,
    /// Channels joined since last sync.
    pub channels_added: Vec<(String, HybridTimestamp)>,
    /// Channels parted since last sync.
    pub channels_removed: Vec<(String, HybridTimestamp)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ServerId;

    fn make_user(uid: &str, nick: &str, server: &ServerId, millis: i64) -> UserCrdt {
        let ts = HybridTimestamp::new(millis, 0, server);
        UserCrdt::new(
            uid.to_string(),
            nick.to_string(),
            "user".to_string(),
            "Real Name".to_string(),
            "host.com".to_string(),
            "cloak.host".to_string(),
            ts,
        )
    }

    #[test]
    fn test_user_crdt_new() {
        let server = ServerId::new("001");
        let user = make_user("001AAA", "TestNick", &server, 100);

        assert_eq!(user.uid, "001AAA");
        assert_eq!(user.nick.value(), "TestNick");
        assert_eq!(user.user.value(), "user");
        assert_eq!(user.realname.value(), "Real Name");
        assert_eq!(user.host.value(), "host.com");
        assert_eq!(user.visible_host.value(), "cloak.host");
        assert!(user.account.value().is_none());
        assert!(user.away.value().is_none());
        assert!(user.channels.is_empty());
    }

    #[test]
    fn test_user_crdt_nick_change() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server2);

        let mut user1 = UserCrdt::new(
            "001AAA".to_string(),
            "OldNick".to_string(),
            "user".to_string(),
            "Real Name".to_string(),
            "host.com".to_string(),
            "cloak.host".to_string(),
            ts1,
        );

        let mut user2 = user1.clone();
        user2.set_nick("NewNick".to_string(), ts2);

        user1.merge(&user2);
        assert_eq!(user1.nick.value(), "NewNick");
    }

    #[test]
    fn test_user_crdt_set_away() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts_away = HybridTimestamp::new(200, 0, &server);
        user.set_away(Some("Gone fishing".to_string()), ts_away);

        assert_eq!(user.away.value(), &Some("Gone fishing".to_string()));
    }

    #[test]
    fn test_user_crdt_clear_away() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts_away = HybridTimestamp::new(200, 0, &server);
        user.set_away(Some("Gone".to_string()), ts_away);

        let ts_back = HybridTimestamp::new(300, 0, &server);
        user.set_away(None, ts_back);

        assert!(user.away.value().is_none());
    }

    #[test]
    fn test_user_crdt_identify() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts_identify = HybridTimestamp::new(200, 0, &server);
        user.identify("MyAccount".to_string(), ts_identify);

        assert_eq!(user.account.value(), &Some("MyAccount".to_string()));
        assert!(*user.modes.registered.value());
    }

    #[test]
    fn test_user_crdt_join_part_channel() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts_join = HybridTimestamp::new(200, 0, &server);
        user.join_channel("#test".to_string(), ts_join);
        assert!(user.channels.contains(&"#test".to_string()));

        let ts_part = HybridTimestamp::new(300, 0, &server);
        user.part_channel("#test", ts_part);
        assert!(!user.channels.contains(&"#test".to_string()));
    }

    #[test]
    fn test_user_crdt_channels_set() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        user.join_channel("#foo".to_string(), ts);
        user.join_channel("#bar".to_string(), ts);

        let channels = user.channels_set();
        assert_eq!(channels.len(), 2);
        assert!(channels.contains("#foo"));
        assert!(channels.contains("#bar"));
    }

    #[test]
    fn test_user_crdt_concurrent_channel_join() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);

        let mut user1 = UserCrdt::new(
            "001AAA".to_string(),
            "Nick".to_string(),
            "user".to_string(),
            "Real".to_string(),
            "host".to_string(),
            "cloak".to_string(),
            ts1,
        );

        // User joins #foo on server1
        user1.join_channel("#foo".to_string(), HybridTimestamp::new(150, 0, &server1));

        let mut user2 = user1.clone();

        // Concurrently, user joins #bar on server2
        user2.join_channel("#bar".to_string(), HybridTimestamp::new(150, 0, &server2));

        // Merge: both channels should be present
        user1.merge(&user2);
        assert!(user1.channels.contains(&"#foo".to_string()));
        assert!(user1.channels.contains(&"#bar".to_string()));
    }

    #[test]
    fn test_user_modes_crdt_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts_base = HybridTimestamp::new(100, 0, &server1);
        let ts1 = HybridTimestamp::new(200, 0, &server1);
        let ts2 = HybridTimestamp::new(300, 0, &server2);

        let mut modes1 = UserModesCrdt::new(ts_base);
        modes1.invisible.update(true, ts1);

        let mut modes2 = UserModesCrdt::new(ts_base);
        modes2.oper.update(true, ts2);

        modes1.merge(&modes2);

        assert!(*modes1.invisible.value());
        assert!(*modes1.oper.value());
    }

    #[test]
    fn test_user_modes_crdt_dominates() {
        let server = ServerId::new("001");
        let ts_base = HybridTimestamp::new(100, 0, &server);
        let ts_later = HybridTimestamp::new(200, 0, &server);

        let modes1 = UserModesCrdt::new(ts_base);
        let modes2 = UserModesCrdt::new(ts_later);

        assert!(modes2.dominates(&modes1));
        assert!(!modes1.dominates(&modes2));
    }

    #[test]
    fn test_user_crdt_dominates() {
        let server = ServerId::new("001");
        let ts_nick = HybridTimestamp::new(200, 0, &server);

        let user1 = make_user("001AAA", "Nick", &server, 100);

        let mut user2 = user1.clone();
        user2.set_nick("NewNick".to_string(), ts_nick);

        assert!(!user1.dominates(&user2));
        assert!(user2.dominates(&user1));
    }

    #[test]
    fn test_user_crdt_snomasks() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        user.modes.snomasks.add('c', ts);
        user.modes.snomasks.add('k', ts);

        assert!(user.modes.snomasks.contains(&'c'));
        assert!(user.modes.snomasks.contains(&'k'));
        assert!(!user.modes.snomasks.contains(&'x'));
    }

    #[test]
    fn test_user_crdt_silence_list() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        user.silence_list.add("*!*@annoying.host".to_string(), ts);

        assert!(user.silence_list.contains(&"*!*@annoying.host".to_string()));
    }

    #[test]
    fn test_user_crdt_accept_list() {
        let server = ServerId::new("001");
        let mut user = make_user("001AAA", "Nick", &server, 100);

        let ts = HybridTimestamp::new(200, 0, &server);
        user.accept_list.add("friend".to_string(), ts);

        assert!(user.accept_list.contains(&"friend".to_string()));
    }
}
