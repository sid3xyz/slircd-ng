//! User-related types and state.

use slirc_crdt::clock::HybridTimestamp;
use slirc_crdt::traits::LwwRegister;
use slirc_crdt::user::{UserCrdt, UserModesCrdt};
use std::collections::HashSet;
use uuid::Uuid;

/// A connected user.
#[derive(Debug)]
pub struct User {
    pub uid: String,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
    /// Real IP address of the connection.
    pub ip: String,
    /// Visible hostname shown to other users (cloaked for privacy).
    pub visible_host: String,
    /// Unique session identifier for this connection (guards against ghost joins).
    pub session_id: Uuid,
    /// Channels this user is in (lowercase names).
    pub channels: HashSet<String>,
    /// User modes.
    pub modes: UserModes,
    /// Account name if identified to NickServ.
    pub account: Option<String>,
    /// Away message if user is marked away (RFC 2812).
    pub away: Option<String>,
    /// IRCv3 capabilities negotiated by this client.
    pub caps: HashSet<String>,
    /// TLS certificate fingerprint (SHA-256 hex) if client presented one.
    pub certfp: Option<String>,
    /// SILENCE list: masks of users to ignore (server-side ignore).
    pub silence_list: HashSet<String>,
    /// ACCEPT list: nicknames allowed to PM even if +R is set (Caller ID).
    pub accept_list: HashSet<String>,
    /// Unix timestamp when this user connected (for S2S UID burst).
    pub created_at: i64,
    /// Last modified timestamp for CRDT synchronization.
    pub last_modified: HybridTimestamp,
}

/// User modes.
#[derive(Debug, Default, Clone)]
pub struct UserModes {
    pub invisible: bool,       // +i
    pub wallops: bool,         // +w
    pub oper: bool,            // +o (IRC operator)
    pub registered: bool,      // +r (identified to NickServ)
    pub secure: bool,          // +Z (TLS connection)
    pub registered_only: bool, // +R (only registered users can PM)
    pub no_ctcp: bool,         // +T (block CTCP except ACTION)
    pub bot: bool,             // +B (marked as a bot)
    pub service: bool,         // +S (network service - protected)
    /// +s - Server notices with granular snomasks (c, r, k, o, etc.)
    /// Empty set means no server notices
    pub snomasks: HashSet<char>,
    /// Operator type (e.g., "admin", "oper") for privilege differentiation.
    /// None means not an operator, Some("oper") for regular opers,
    /// Some("admin") for server admins.
    pub oper_type: Option<String>,
}

impl UserModes {
    /// Convert modes to a string like "+iw".
    pub fn as_mode_string(&self) -> String {
        let mut s = String::from("+");
        if self.invisible {
            s.push('i');
        }
        if self.wallops {
            s.push('w');
        }
        if self.oper {
            s.push('o');
        }
        if self.registered {
            s.push('r');
        }
        if self.secure {
            s.push('Z');
        }
        if self.registered_only {
            s.push('R');
        }
        if self.no_ctcp {
            s.push('T');
        }
        if self.bot {
            s.push('B');
        }
        if self.service {
            s.push('S');
        }
        if !self.snomasks.is_empty() {
            s.push('s');
        }
        if s == "+" { "+".to_string() } else { s }
    }

    /// Check if user has a specific snomask.
    pub fn has_snomask(&self, mask: char) -> bool {
        self.snomasks.contains(&mask)
    }

    /// Create UserModes from a CRDT representation.
    pub fn from_crdt(crdt: &UserModesCrdt) -> Self {
        Self {
            invisible: *crdt.invisible.value(),
            wallops: *crdt.wallops.value(),
            oper: *crdt.oper.value(),
            registered: *crdt.registered.value(),
            secure: *crdt.secure.value(),
            registered_only: *crdt.registered_only.value(),
            no_ctcp: *crdt.no_ctcp.value(),
            bot: *crdt.bot.value(),
            snomasks: crdt.snomasks.iter().cloned().collect(),
            oper_type: crdt.oper_type.value().clone(),
            service: false, // Services are never remote users
        }
    }

    /// Convert to CRDT representation.
    pub fn to_crdt(&self, timestamp: HybridTimestamp) -> UserModesCrdt {
        let mut crdt = UserModesCrdt::new(timestamp);
        crdt.invisible = LwwRegister::new(self.invisible, timestamp);
        crdt.wallops = LwwRegister::new(self.wallops, timestamp);
        crdt.oper = LwwRegister::new(self.oper, timestamp);
        crdt.registered = LwwRegister::new(self.registered, timestamp);
        crdt.secure = LwwRegister::new(self.secure, timestamp);
        crdt.registered_only = LwwRegister::new(self.registered_only, timestamp);
        crdt.no_ctcp = LwwRegister::new(self.no_ctcp, timestamp);
        crdt.bot = LwwRegister::new(self.bot, timestamp);
        for &mask in &self.snomasks {
            crdt.snomasks.add(mask, timestamp);
        }
        crdt.oper_type = LwwRegister::new(self.oper_type.clone(), timestamp);
        crdt
    }
}

/// Parameters for creating a new User.
#[derive(Debug)]
pub struct UserParams {
    pub uid: String,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
    pub ip: String,
    pub cloak_secret: String,
    pub cloak_suffix: String,
    pub caps: HashSet<String>,
    pub certfp: Option<String>,
    pub last_modified: HybridTimestamp,
}

impl User {
    /// Create a new user.
    ///
    /// The `host` is cloaked using HMAC-SHA256 with the provided secret.
    /// `caps` is the set of IRCv3 capabilities negotiated during handshake.
    /// `certfp` is the TLS client certificate fingerprint, if any.
    pub fn new(params: UserParams) -> Self {
        let UserParams {
            uid,
            nick,
            user,
            realname,
            host,
            ip,
            cloak_secret,
            cloak_suffix,
            caps,
            certfp,
            last_modified,
        } = params;

        // Try to parse as IP for proper cloaking, fall back to hostname cloaking
        let visible_host = if let Ok(addr) = ip.parse::<std::net::IpAddr>() {
            crate::security::cloaking::cloak_ip_hmac_with_suffix(
                &addr,
                &cloak_secret,
                &cloak_suffix,
            )
        } else {
            crate::security::cloaking::cloak_hostname(&host, &cloak_secret)
        };
        Self {
            uid,
            nick,
            user,
            realname,
            host,
            ip,
            visible_host,
            session_id: Uuid::new_v4(),
            channels: HashSet::new(),
            modes: UserModes::default(),
            account: None,
            away: None,
            caps,
            certfp,
            silence_list: HashSet::new(),
            accept_list: HashSet::new(),
            created_at: chrono::Utc::now().timestamp(),
            last_modified,
        }
    }

    /// Convert to CRDT representation.
    pub fn to_crdt(&self) -> UserCrdt {
        let mut crdt = UserCrdt::new(
            self.uid.clone(),
            self.nick.clone(),
            self.user.clone(),
            self.realname.clone(),
            self.host.clone(),
            self.visible_host.clone(),
            self.last_modified,
        );
        crdt.account = LwwRegister::new(self.account.clone(), self.last_modified);
        crdt.away = LwwRegister::new(self.away.clone(), self.last_modified);
        for chan in &self.channels {
            crdt.channels.add(chan.clone(), self.last_modified);
        }
        for cap in &self.caps {
            crdt.caps.add(cap.clone(), self.last_modified);
        }
        crdt.modes = self.modes.to_crdt(self.last_modified);
        for mask in &self.silence_list {
            crdt.silence_list.add(mask.clone(), self.last_modified);
        }
        for nick in &self.accept_list {
            crdt.accept_list.add(nick.clone(), self.last_modified);
        }
        crdt
    }

    /// Create a User from a CRDT representation.
    pub fn from_crdt(crdt: UserCrdt) -> Self {
        let last_modified = crdt.nick.timestamp();
        Self {
            uid: crdt.uid.clone(),
            nick: crdt.nick.value().clone(),
            user: crdt.user.value().clone(),
            realname: crdt.realname.value().clone(),
            host: crdt.host.value().clone(),
            ip: "0.0.0.0".to_string(), // Remote users don't have local IP
            visible_host: crdt.visible_host.value().clone(),
            session_id: Uuid::nil(), // Remote users don't have local session
            channels: crdt.channels.iter().cloned().collect(),
            modes: UserModes::from_crdt(&crdt.modes),
            account: crdt.account.value().clone(),
            away: crdt.away.value().clone(),
            caps: crdt.caps.iter().cloned().collect(),
            certfp: None,
            silence_list: crdt.silence_list.iter().cloned().collect(),
            accept_list: crdt.accept_list.iter().cloned().collect(),
            created_at: last_modified.millis / 1000, // Convert from HybridTimestamp millis
            last_modified,
        }
    }

    pub fn merge_crdt(&mut self, incoming: UserCrdt) {
        use slirc_crdt::Crdt;

        let mut merged = self.to_crdt();
        merged.merge(&incoming);

        self.nick = merged.nick.value().clone();
        self.user = merged.user.value().clone();
        self.realname = merged.realname.value().clone();
        self.host = merged.host.value().clone();
        self.visible_host = merged.visible_host.value().clone();
        self.channels = merged.channels.iter().cloned().collect();
        self.modes = UserModes::from_crdt(&merged.modes);
        self.account = merged.account.value().clone();
        self.away = merged.away.value().clone();
        self.caps = merged.caps.iter().cloned().collect();
        self.silence_list = merged.silence_list.iter().cloned().collect();
        self.accept_list = merged.accept_list.iter().cloned().collect();

        self.last_modified = merged.nick.timestamp();

        // Remote users have a nil session_id; for them, keep created_at derived from CRDT.
        // For local users, preserve the local created_at.
        if self.session_id.is_nil() {
            self.created_at = self.last_modified.millis / 1000;
        }
    }
}

/// An entry in the WHOWAS history for a disconnected user.
#[derive(Debug, Clone)]
pub struct WhowasEntry {
    /// The user's nickname (case-preserved).
    pub nick: String,
    /// The user's username.
    pub user: String,
    /// The user's hostname.
    pub host: String,
    /// The user's realname.
    pub realname: String,
    /// Server name they were connected to.
    pub server: String,
    /// When they logged out (Unix timestamp in milliseconds).
    pub logout_time: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== UserModes::as_mode_string tests ==========

    #[test]
    fn mode_string_default_returns_plus() {
        let modes = UserModes::default();
        assert_eq!(modes.as_mode_string(), "+");
    }

    #[test]
    fn mode_string_invisible_only() {
        let modes = UserModes {
            invisible: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+i");
    }

    #[test]
    fn mode_string_wallops_only() {
        let modes = UserModes {
            wallops: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+w");
    }

    #[test]
    fn mode_string_oper_only() {
        let modes = UserModes {
            oper: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+o");
    }

    #[test]
    fn mode_string_registered_only() {
        let modes = UserModes {
            registered: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+r");
    }

    #[test]
    fn mode_string_secure_only() {
        let modes = UserModes {
            secure: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+Z");
    }

    #[test]
    fn mode_string_registered_only_flag() {
        let modes = UserModes {
            registered_only: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+R");
    }

    #[test]
    fn mode_string_no_ctcp_only() {
        let modes = UserModes {
            no_ctcp: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+T");
    }

    #[test]
    fn mode_string_bot_only() {
        let modes = UserModes {
            bot: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+B");
    }

    #[test]
    fn mode_string_service_only() {
        let modes = UserModes {
            service: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+S");
    }

    #[test]
    fn mode_string_snomasks_appends_s() {
        let mut snomasks = HashSet::new();
        snomasks.insert('c');
        let modes = UserModes {
            snomasks,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+s");
    }

    #[test]
    fn mode_string_multiple_modes() {
        let modes = UserModes {
            invisible: true,
            wallops: true,
            oper: true,
            ..Default::default()
        };
        assert_eq!(modes.as_mode_string(), "+iwo");
    }

    #[test]
    fn mode_string_all_modes_set() {
        let mut snomasks = HashSet::new();
        snomasks.insert('c');
        snomasks.insert('r');
        let modes = UserModes {
            invisible: true,
            wallops: true,
            oper: true,
            registered: true,
            secure: true,
            registered_only: true,
            no_ctcp: true,
            bot: true,
            service: true,
            snomasks,
            oper_type: Some("admin".to_string()),
        };
        assert_eq!(modes.as_mode_string(), "+iworZRTBSs");
    }

    // ========== UserModes::has_snomask tests ==========

    #[test]
    fn has_snomask_empty_returns_false() {
        let modes = UserModes::default();
        assert!(!modes.has_snomask('c'));
        assert!(!modes.has_snomask('r'));
    }

    #[test]
    fn has_snomask_with_c_returns_true_for_c() {
        let mut snomasks = HashSet::new();
        snomasks.insert('c');
        let modes = UserModes {
            snomasks,
            ..Default::default()
        };
        assert!(modes.has_snomask('c'));
        assert!(!modes.has_snomask('r'));
    }

    #[test]
    fn has_snomask_multiple_masks() {
        let mut snomasks = HashSet::new();
        snomasks.insert('c');
        snomasks.insert('r');
        snomasks.insert('k');
        let modes = UserModes {
            snomasks,
            ..Default::default()
        };
        assert!(modes.has_snomask('c'));
        assert!(modes.has_snomask('r'));
        assert!(modes.has_snomask('k'));
        assert!(!modes.has_snomask('o'));
    }
}
