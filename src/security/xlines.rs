//! X-Lines and Extended Bans for server-level moderation.
//!
//! Provides:
//! - **X-Lines**: K-line, G-line, Z-line, R-line, S-line server bans
//! - **Extended Bans**: Pattern matching beyond nick!user@host (accounts, realnames, etc.)
//!
//! # X-Line Types
//!
//! | Type | Scope | Match Pattern |
//! |------|-------|---------------|
//! | K-Line | Local | nick!user@host |
//! | G-Line | Global | nick!user@host |
//! | Z-Line | Global | IP address |
//! | R-Line | Global | Regex on nick!user@host realname |
//! | S-Line | Global | Server name |
//!
//! # Extended Ban Types
//!
//! | Prefix | Description |
//! |--------|-------------|
//! | `$a:` | Account name |
//! | `$r:` | Realname field |
//! | `$s:` | Server name |
//! | `$c:` | Channel membership |
//! | `$o:` | Operator type |
//! | `$x:` | Certificate fingerprint |
//! | `$z:` | SASL mechanism |
//! | `$U` | Unregistered users |

use regex::Regex;
use std::net::IpAddr;
use std::time::SystemTime;

/// Extended Ban types for advanced pattern matching.
///
/// These extend beyond simple nick!user@host to match on various user attributes.
/// Will be used in Phase 3 for +b extended ban support.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ExtendedBan {
    /// `$a:account` - Matches users logged into a specific account.
    Account(String),
    /// `$r:pattern` - Matches user's realname field.
    Realname(String),
    /// `$s:server` - Matches user's connected server.
    Server(String),
    /// `$c:channel` - Matches users in a specific channel.
    Channel(String),
    /// `$o:type` - Matches IRC operators of a given type.
    Oper(String),
    /// `$x:fingerprint` - Matches SSL certificate fingerprint.
    Certificate(String),
    /// `$z:mechanism` - Matches SASL authentication mechanism.
    Sasl(String),
    /// `$j:pattern` - Matches channel join patterns.
    Join(String),
    /// `$U` - Matches unregistered (not identified) users.
    Unregistered,
}

#[allow(dead_code)]
impl ExtendedBan {
    /// Parse extended ban from string format like "$a:nickname" or "$r:*bot*".
    ///
    /// Returns `None` if the string is not a valid extended ban format.
    pub fn parse(ban_string: &str) -> Option<Self> {
        if !ban_string.starts_with('$') {
            return None;
        }

        let parts: Vec<&str> = ban_string.splitn(2, ':').collect();
        if parts.len() < 2 {
            // Handle special cases like $U (unregistered)
            return match ban_string {
                "$U" => Some(ExtendedBan::Unregistered),
                _ => None,
            };
        }

        let ban_type = parts[0];
        let pattern = parts[1].to_string();

        match ban_type {
            "$a" => Some(ExtendedBan::Account(pattern)),
            "$r" => Some(ExtendedBan::Realname(pattern)),
            "$s" => Some(ExtendedBan::Server(pattern)),
            "$c" => Some(ExtendedBan::Channel(pattern)),
            "$o" => Some(ExtendedBan::Oper(pattern)),
            "$x" => Some(ExtendedBan::Certificate(pattern)),
            "$z" => Some(ExtendedBan::Sasl(pattern)),
            "$j" => Some(ExtendedBan::Join(pattern)),
            _ => None,
        }
    }

    /// Convert extended ban back to string format.
    pub fn to_ban_string(&self) -> String {
        match self {
            ExtendedBan::Account(p) => format!("$a:{}", p),
            ExtendedBan::Realname(p) => format!("$r:{}", p),
            ExtendedBan::Server(p) => format!("$s:{}", p),
            ExtendedBan::Channel(p) => format!("$c:{}", p),
            ExtendedBan::Oper(p) => format!("$o:{}", p),
            ExtendedBan::Certificate(p) => format!("$x:{}", p),
            ExtendedBan::Sasl(p) => format!("$z:{}", p),
            ExtendedBan::Join(p) => format!("$j:{}", p),
            ExtendedBan::Unregistered => "$U".to_string(),
        }
    }
}

/// User context for evaluating extended bans and X-lines.
#[derive(Debug, Clone)]
pub struct UserContext {
    /// User's current nickname.
    pub nickname: String,
    /// User's username (ident).
    pub username: String,
    /// User's hostname (may be cloaked).
    pub hostname: String,
    /// User's realname (GECOS).
    pub realname: String,
    /// Account name if identified to NickServ.
    pub account: Option<String>,
    /// Server the user is connected to.
    pub server: String,
    /// Channels the user is in (lowercase).
    pub channels: Vec<String>,
    /// Whether the user is an IRC operator.
    pub is_oper: bool,
    /// Type of operator (e.g., "admin", "oper").
    pub oper_type: Option<String>,
    /// TLS certificate fingerprint if available.
    pub certificate_fp: Option<String>,
    /// SASL mechanism used for authentication.
    pub sasl_mechanism: Option<String>,
    /// User's real IP address.
    pub ip_address: IpAddr,
    /// Whether the user has identified to an account.
    pub is_registered: bool,
}

impl UserContext {
    /// Create a minimal context for connection-time checks (before NICK/USER).
    /// Used for Z-line checks at connection time.
    #[allow(dead_code)] // Will be used when we add early Z-line checks
    pub fn for_connection(ip: IpAddr, hostname: String) -> Self {
        Self {
            nickname: "*".to_string(),
            username: "*".to_string(),
            hostname,
            realname: String::new(),
            account: None,
            server: String::new(),
            channels: Vec::new(),
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            ip_address: ip,
            is_registered: false,
        }
    }

    /// Create a context for registration-time checks (after NICK/USER, before welcome).
    pub fn for_registration(
        ip: IpAddr,
        hostname: String,
        nickname: String,
        username: String,
        realname: String,
        server: String,
        account: Option<String>,
    ) -> Self {
        Self {
            nickname,
            username,
            hostname,
            realname,
            account: account.clone(),
            server,
            channels: Vec::new(),
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            ip_address: ip,
            is_registered: account.is_some(),
        }
    }

    /// Get the full hostmask (nick!user@host).
    #[allow(dead_code)] // Will be used in Phase 3b for logging
    pub fn hostmask(&self) -> String {
        format!("{}!{}@{}", self.nickname, self.username, self.hostname)
    }
}

/// X-Line ban types following traditional IRC server conventions.
/// Variants are constructed by admin commands (KLINE, GLINE, ZLINE) in Phase 3b.
#[allow(dead_code)] // Constructed by admin commands
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)] // Traditional IRC naming convention: K-Line, G-Line, etc.
pub enum XLine {
    /// K-Line: Local user ban by hostmask.
    KLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
        set_by: String,
        set_at: SystemTime,
    },
    /// G-Line: Global user ban by hostmask.
    GLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
        set_by: String,
        set_at: SystemTime,
    },
    /// Z-Line: IP address ban (no DNS lookup required).
    ZLine {
        ip: String,
        reason: String,
        expires: Option<SystemTime>,
        set_by: String,
        set_at: SystemTime,
    },
    /// R-Line: Regex-based ban on nick!user@host + realname.
    #[allow(dead_code)] // Phase 4: Regex bans
    RLine {
        regex: String,
        reason: String,
        expires: Option<SystemTime>,
        set_by: String,
        set_at: SystemTime,
    },
    /// S-Line: Server ban (prevents server linking).
    #[allow(dead_code)] // Phase 4: Server linking
    SLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
        set_by: String,
        set_at: SystemTime,
    },
}

impl XLine {
    /// Check if this X-line has expired.
    pub fn is_expired(&self) -> bool {
        let expires = match self {
            XLine::KLine { expires, .. }
            | XLine::GLine { expires, .. }
            | XLine::ZLine { expires, .. }
            | XLine::RLine { expires, .. }
            | XLine::SLine { expires, .. } => *expires,
        };

        if let Some(expiry) = expires {
            SystemTime::now() > expiry
        } else {
            false // Permanent ban (no expiry)
        }
    }

    /// Get the pattern/mask for this X-line.
    #[allow(dead_code)] // Used by admin commands in Phase 3b
    pub fn pattern(&self) -> &str {
        match self {
            XLine::KLine { mask, .. } | XLine::GLine { mask, .. } => mask,
            XLine::ZLine { ip, .. } => ip,
            XLine::RLine { regex, .. } => regex,
            XLine::SLine { mask, .. } => mask,
        }
    }

    /// Get the reason for this X-line.
    pub fn reason(&self) -> &str {
        match self {
            XLine::KLine { reason, .. }
            | XLine::GLine { reason, .. }
            | XLine::ZLine { reason, .. }
            | XLine::RLine { reason, .. }
            | XLine::SLine { reason, .. } => reason,
        }
    }

    /// Get the type name of this X-line.
    pub fn type_name(&self) -> &'static str {
        match self {
            XLine::KLine { .. } => "K-Line",
            XLine::GLine { .. } => "G-Line",
            XLine::ZLine { .. } => "Z-Line",
            XLine::RLine { .. } => "R-Line",
            XLine::SLine { .. } => "S-Line",
        }
    }
}

/// Check if an extended ban matches a user context.
pub fn matches_extended_ban(ban: &ExtendedBan, context: &UserContext) -> bool {
    match ban {
        ExtendedBan::Account(pattern) => {
            if let Some(account) = &context.account {
                wildcard_match(pattern, account)
            } else {
                false
            }
        }
        ExtendedBan::Realname(pattern) => wildcard_match(pattern, &context.realname),
        ExtendedBan::Server(pattern) => wildcard_match(pattern, &context.server),
        ExtendedBan::Channel(pattern) => context
            .channels
            .iter()
            .any(|chan| wildcard_match(pattern, chan)),
        ExtendedBan::Oper(pattern) => {
            if context.is_oper {
                if let Some(oper_type) = &context.oper_type {
                    wildcard_match(pattern, oper_type)
                } else {
                    pattern == "*" // Match any oper if no specific type
                }
            } else {
                false
            }
        }
        ExtendedBan::Certificate(pattern) => {
            if let Some(cert_fp) = &context.certificate_fp {
                wildcard_match(pattern, cert_fp)
            } else {
                false
            }
        }
        ExtendedBan::Sasl(pattern) => {
            if let Some(sasl) = &context.sasl_mechanism {
                wildcard_match(pattern, sasl)
            } else {
                false
            }
        }
        ExtendedBan::Join(pattern) => {
            // Match against nickname for join patterns
            wildcard_match(pattern, &context.nickname)
        }
        ExtendedBan::Unregistered => !context.is_registered,
    }
}

/// Check if an X-line matches a user context.
pub fn matches_xline(xline: &XLine, context: &UserContext) -> bool {
    if xline.is_expired() {
        return false;
    }

    match xline {
        XLine::KLine { mask, .. } | XLine::GLine { mask, .. } => {
            // Check both nick!user@ip and nick!user@hostname
            let user_mask_ip = format!(
                "{}!{}@{}",
                context.nickname, context.username, context.ip_address
            );
            let user_mask_host = format!(
                "{}!{}@{}",
                context.nickname, context.username, context.hostname
            );
            wildcard_match(mask, &user_mask_ip) || wildcard_match(mask, &user_mask_host)
        }
        XLine::ZLine { ip, .. } => wildcard_match(ip, &context.ip_address.to_string()),
        XLine::RLine { regex, .. } => {
            if let Ok(re) = Regex::new(regex) {
                let user_string = format!(
                    "{}!{}@{} {}",
                    context.nickname, context.username, context.ip_address, context.realname
                );
                re.is_match(&user_string)
            } else {
                false
            }
        }
        XLine::SLine { mask, .. } => wildcard_match(mask, &context.server),
    }
}

/// Simple wildcard matching with `*` and `?` support.
///
/// - `*` matches zero or more characters
/// - `?` matches exactly one character
///
/// Case-insensitive matching for IRC compatibility.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Convert wildcard pattern to regex
    let mut regex_pattern = String::from("(?i)^");
    for c in pattern.chars() {
        match c {
            '*' => regex_pattern.push_str(".*"),
            '?' => regex_pattern.push('.'),
            // Escape regex special characters
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                regex_pattern.push('\\');
                regex_pattern.push(c);
            }
            _ => regex_pattern.push(c),
        }
    }
    regex_pattern.push('$');

    Regex::new(&regex_pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> UserContext {
        UserContext {
            nickname: "TestUser".to_string(),
            username: "testuser".to_string(),
            hostname: "example.com".to_string(),
            realname: "Test User".to_string(),
            account: Some("testaccount".to_string()),
            server: "irc.straylight.net".to_string(),
            channels: vec!["#test".to_string(), "#rust".to_string()],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: Some("PLAIN".to_string()),
            ip_address: "192.168.1.100".parse().unwrap(),
            is_registered: true,
        }
    }

    #[test]
    fn test_extended_ban_parsing() {
        assert!(matches!(
            ExtendedBan::parse("$a:testaccount"),
            Some(ExtendedBan::Account(_))
        ));
        assert!(matches!(
            ExtendedBan::parse("$r:*bot*"),
            Some(ExtendedBan::Realname(_))
        ));
        assert!(matches!(
            ExtendedBan::parse("$U"),
            Some(ExtendedBan::Unregistered)
        ));
        assert!(ExtendedBan::parse("not-a-ban").is_none());
        assert!(ExtendedBan::parse("$x").is_none()); // Missing pattern
    }

    #[test]
    fn test_extended_ban_roundtrip() {
        let bans = vec![
            "$a:testaccount",
            "$r:*bot*",
            "$s:*.freenode.net",
            "$U",
        ];
        for ban_str in bans {
            let ban = ExtendedBan::parse(ban_str).unwrap();
            assert_eq!(ban.to_ban_string(), ban_str);
        }
    }

    #[test]
    fn test_account_ban_match() {
        let context = test_context();
        let ban = ExtendedBan::Account("testaccount".to_string());
        assert!(matches_extended_ban(&ban, &context));

        let ban_nomatch = ExtendedBan::Account("otheraccount".to_string());
        assert!(!matches_extended_ban(&ban_nomatch, &context));
    }

    #[test]
    fn test_unregistered_ban() {
        let mut context = test_context();
        let ban = ExtendedBan::Unregistered;

        // Registered user should not match
        assert!(!matches_extended_ban(&ban, &context));

        // Unregistered user should match
        context.is_registered = false;
        assert!(matches_extended_ban(&ban, &context));
    }

    #[test]
    fn test_channel_ban() {
        let context = test_context();
        let ban = ExtendedBan::Channel("#test".to_string());
        assert!(matches_extended_ban(&ban, &context));

        let ban_nomatch = ExtendedBan::Channel("#secret".to_string());
        assert!(!matches_extended_ban(&ban_nomatch, &context));
    }

    #[test]
    fn test_wildcard_matching() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("test*", "testing"));
        assert!(wildcard_match("*test", "unittest"));
        assert!(wildcard_match("*test*", "unittesting"));
        assert!(wildcard_match("te?t", "test"));
        assert!(!wildcard_match("te?t", "tests"));
        assert!(wildcard_match("*.example.com", "user.example.com"));
    }

    #[test]
    fn test_wildcard_case_insensitive() {
        assert!(wildcard_match("TEST*", "testing"));
        assert!(wildcard_match("test*", "TESTING"));
    }

    #[test]
    fn test_xline_expiry() {
        use std::time::Duration;

        let now = SystemTime::now();
        let expired = XLine::KLine {
            mask: "*!*@example.com".to_string(),
            reason: "Test".to_string(),
            expires: Some(now - Duration::from_secs(60)),
            set_by: "admin".to_string(),
            set_at: now - Duration::from_secs(120),
        };
        assert!(expired.is_expired());

        let active = XLine::KLine {
            mask: "*!*@example.com".to_string(),
            reason: "Test".to_string(),
            expires: Some(now + Duration::from_secs(3600)),
            set_by: "admin".to_string(),
            set_at: now,
        };
        assert!(!active.is_expired());

        let permanent = XLine::KLine {
            mask: "*!*@example.com".to_string(),
            reason: "Test".to_string(),
            expires: None, // Permanent
            set_by: "admin".to_string(),
            set_at: now,
        };
        assert!(!permanent.is_expired());
    }

    #[test]
    fn test_zline_match() {
        let now = SystemTime::now();
        let context = test_context();

        let zline = XLine::ZLine {
            ip: "192.168.1.*".to_string(),
            reason: "Banned subnet".to_string(),
            expires: None,
            set_by: "admin".to_string(),
            set_at: now,
        };
        assert!(matches_xline(&zline, &context));

        let zline_nomatch = XLine::ZLine {
            ip: "10.0.0.*".to_string(),
            reason: "Different subnet".to_string(),
            expires: None,
            set_by: "admin".to_string(),
            set_at: now,
        };
        assert!(!matches_xline(&zline_nomatch, &context));
    }
}
