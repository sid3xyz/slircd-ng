//! User-related types and state.

use std::collections::HashSet;

/// A connected user.
#[derive(Debug)]
pub struct User {
    pub uid: String,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
    /// Visible hostname shown to other users (cloaked for privacy).
    pub visible_host: String,
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
    /// +s - Server notices with granular snomasks (c, r, k, o, etc.)
    /// Empty set means no server notices
    pub snomasks: HashSet<char>,
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
        if !self.snomasks.is_empty() {
            s.push('s');
        }
        if s == "+" { "+".to_string() } else { s }
    }

    /// Check if user has a specific snomask.
    pub fn has_snomask(&self, mask: char) -> bool {
        self.snomasks.contains(&mask)
    }

    /// Add a snomask.
    pub fn add_snomask(&mut self, mask: char) {
        self.snomasks.insert(mask);
    }

    /// Remove a snomask.
    pub fn remove_snomask(&mut self, mask: char) {
        self.snomasks.remove(&mask);
    }
}

impl User {
    /// Create a new user.
    ///
    /// The `host` is cloaked using HMAC-SHA256 with the provided secret.
    /// `caps` is the set of IRCv3 capabilities negotiated during handshake.
    /// `certfp` is the TLS client certificate fingerprint, if any.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uid: String,
        nick: String,
        user: String,
        realname: String,
        host: String,
        cloak_secret: &str,
        cloak_suffix: &str,
        caps: HashSet<String>,
        certfp: Option<String>,
    ) -> Self {
        // Try to parse as IP for proper cloaking, fall back to hostname cloaking
        let visible_host = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            crate::security::cloaking::cloak_ip_hmac_with_suffix(&ip, cloak_secret, cloak_suffix)
        } else {
            crate::security::cloaking::cloak_hostname(&host, cloak_secret)
        };
        Self {
            uid,
            nick,
            user,
            realname,
            host,
            visible_host,
            channels: HashSet::new(),
            modes: UserModes::default(),
            account: None,
            away: None,
            caps,
            certfp,
            silence_list: HashSet::new(),
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
    /// When they logged out (Unix timestamp).
    pub logout_time: i64,
}
