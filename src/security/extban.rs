//! Extended Ban Types and X-Lines
//!
//! Advanced anti-abuse features for IRC server:
//! - **Extended bans (EXTBAN)**: Pattern matching beyond nick!user@host
//! - **X-lines**: Server-level bans (K/G/Z/R/S-line)
//!
//! # Extended Ban Types
//!
//! Modern IRC networks require sophisticated abuse prevention beyond simple
//! nick!user@host matching. Extended bans provide powerful pattern matching
//! for account names, realnames, servers, and certificates.
//!
//! Supported formats:
//! - `$a:account` - matches users logged into account
//! - `$r:pattern` - matches realname field
//! - `$s:server` - matches user's server
//! - `$c:channel` - matches users in channel
//! - `$o:type` - matches IRCops of given type
//! - `$x:fp` - matches SSL certificate fingerprint
//! - `$U` - matches unregistered users
//! - `$z:pattern` - matches SASL authentication mechanism
//! - `$j:pattern` - matches channel join patterns

// Allow dead_code for Phase 1: types will be integrated in Phase 2
#![allow(dead_code)]

use std::net::IpAddr;
use std::time::SystemTime;

/// Extended Ban Types for advanced pattern matching
///
/// Extends beyond simple nick!user@host to match on various user attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtendedBan {
    /// $a:account - matches users logged into account
    Account(String),
    /// $r:pattern - matches realname field
    Realname(String),
    /// $s:server - matches user's server
    Server(String),
    /// $c:channel - matches users in channel
    Channel(String),
    /// $o:type - matches IRCops of given type
    Oper(String),
    /// $x:fp - matches SSL certificate fingerprint
    Certificate(String),
    /// $U - matches unregistered users
    Unregistered,
    /// $z:pattern - matches SASL authentication mechanism
    Sasl(String),
    /// $j:pattern - matches channel join patterns
    Join(String),
}

impl ExtendedBan {
    /// Parse extended ban from string format like "$a:nickname" or "$r:*bot*"
    ///
    /// # Examples
    ///
    /// ```
    /// # use slircd_ng::security::extban::ExtendedBan;
    /// let ban = ExtendedBan::parse("$a:spammer").unwrap();
    /// assert!(matches!(ban, ExtendedBan::Account(ref s) if s == "spammer"));
    ///
    /// let ban = ExtendedBan::parse("$U").unwrap();
    /// assert_eq!(ban, ExtendedBan::Unregistered);
    /// ```
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

    /// Serialize extended ban to string format
    pub fn as_string(&self) -> String {
        match self {
            ExtendedBan::Account(pattern) => format!("$a:{}", pattern),
            ExtendedBan::Realname(pattern) => format!("$r:{}", pattern),
            ExtendedBan::Server(pattern) => format!("$s:{}", pattern),
            ExtendedBan::Channel(pattern) => format!("$c:{}", pattern),
            ExtendedBan::Oper(pattern) => format!("$o:{}", pattern),
            ExtendedBan::Certificate(pattern) => format!("$x:{}", pattern),
            ExtendedBan::Unregistered => "$U".to_string(),
            ExtendedBan::Sasl(pattern) => format!("$z:{}", pattern),
            ExtendedBan::Join(pattern) => format!("$j:{}", pattern),
        }
    }

    /// Check if a user context matches this extended ban
    ///
    /// # Arguments
    ///
    /// * `user` - User context containing all relevant fields for matching
    ///
    /// # Returns
    ///
    /// `true` if the ban matches the user, `false` otherwise
    pub fn matches(&self, user: &UserContext) -> bool {
        match self {
            ExtendedBan::Account(pattern) => {
                if let Some(account) = &user.account {
                    wildcard_match(pattern, account)
                } else {
                    false
                }
            }
            ExtendedBan::Realname(pattern) => wildcard_match(pattern, &user.realname),
            ExtendedBan::Server(pattern) => wildcard_match(pattern, &user.server),
            ExtendedBan::Channel(pattern) => user
                .channels
                .iter()
                .any(|ch| wildcard_match(pattern, ch)),
            ExtendedBan::Oper(pattern) => {
                if user.is_oper {
                    if let Some(oper_type) = &user.oper_type {
                        wildcard_match(pattern, oper_type)
                    } else {
                        // If no specific type, match any oper
                        pattern == "*"
                    }
                } else {
                    false
                }
            }
            ExtendedBan::Certificate(pattern) => {
                if let Some(cert_fp) = &user.certificate_fp {
                    wildcard_match(pattern, cert_fp)
                } else {
                    false
                }
            }
            ExtendedBan::Unregistered => !user.is_registered,
            ExtendedBan::Sasl(pattern) => {
                if let Some(mech) = &user.sasl_mechanism {
                    wildcard_match(pattern, mech)
                } else {
                    false
                }
            }
            ExtendedBan::Join(pattern) => {
                // Match against recent join patterns (implementation-specific)
                // For now, just check if pattern matches any channel
                user.channels
                    .iter()
                    .any(|ch| wildcard_match(pattern, ch))
            }
        }
    }
}

/// User context for evaluating extended bans
///
/// Contains all user attributes that can be matched by extended bans.
#[derive(Debug, Clone)]
pub struct UserContext {
    pub nickname: String,
    pub username: String,
    pub hostname: String,
    pub realname: String,
    pub account: Option<String>,
    pub server: String,
    pub channels: Vec<String>,
    pub is_oper: bool,
    pub oper_type: Option<String>,
    pub certificate_fp: Option<String>,
    pub sasl_mechanism: Option<String>,
    pub ip_address: IpAddr,
    pub is_registered: bool,
}

/// X-line ban types following traditional IRC server conventions
///
/// X-lines are server-level bans that can be applied based on various criteria.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum XLine {
    /// K-line: Local user bans (user@host)
    KLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    },
    /// G-line: Global user bans (networked)
    GLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    },
    /// Z-line: IP address bans
    ZLine {
        ip: String,
        reason: String,
        expires: Option<SystemTime>,
    },
    /// R-line: Regex-based bans
    RLine {
        regex: String,
        reason: String,
        expires: Option<SystemTime>,
    },
    /// S-line: Server bans
    SLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    },
}

impl XLine {
    /// Check if X-line has expired
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
            false // Permanent ban
        }
    }

    /// Get the pattern/mask for this X-line (for indexing purposes)
    pub fn pattern(&self) -> &str {
        match self {
            XLine::KLine { mask, .. } | XLine::GLine { mask, .. } => mask,
            XLine::ZLine { ip, .. } => ip,
            XLine::RLine { regex, .. } => regex,
            XLine::SLine { mask, .. } => mask,
        }
    }

    /// Get the reason for this X-line
    pub fn reason(&self) -> &str {
        match self {
            XLine::KLine { reason, .. }
            | XLine::GLine { reason, .. }
            | XLine::ZLine { reason, .. }
            | XLine::RLine { reason, .. }
            | XLine::SLine { reason, .. } => reason,
        }
    }

    /// Get the X-line type as a string
    pub fn line_type(&self) -> &'static str {
        match self {
            XLine::KLine { .. } => "K",
            XLine::GLine { .. } => "G",
            XLine::ZLine { .. } => "Z",
            XLine::RLine { .. } => "R",
            XLine::SLine { .. } => "S",
        }
    }
}

/// Simple wildcard matching for IRC patterns
///
/// Supports `*` (match zero or more chars) and `?` (match exactly one char).
///
/// # Examples
///
/// ```
/// # use slircd_ng::security::extban::wildcard_match;
/// assert!(wildcard_match("*bot*", "mybot123"));
/// assert!(wildcard_match("user?", "user1"));
/// assert!(!wildcard_match("user?", "user12"));
/// ```
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Convert IRC wildcards to regex
    let regex_pattern = pattern
        .replace('.', r"\.")
        .replace('*', ".*")
        .replace('?', ".");

    // Case-insensitive matching for IRC
    if let Ok(re) = regex::Regex::new(&format!("(?i)^{}$", regex_pattern)) {
        re.is_match(text)
    } else {
        // Fallback to exact match if regex compilation fails
        pattern.eq_ignore_ascii_case(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_extban_parse_account() {
        let ban = ExtendedBan::parse("$a:spammer").unwrap();
        assert!(matches!(ban, ExtendedBan::Account(ref s) if s == "spammer"));
    }

    #[test]
    fn test_extban_parse_realname() {
        let ban = ExtendedBan::parse("$r:*bot*").unwrap();
        assert!(matches!(ban, ExtendedBan::Realname(ref s) if s == "*bot*"));
    }

    #[test]
    fn test_extban_parse_unregistered() {
        let ban = ExtendedBan::parse("$U").unwrap();
        assert_eq!(ban, ExtendedBan::Unregistered);
    }

    #[test]
    fn test_extban_parse_invalid() {
        assert!(ExtendedBan::parse("notaextban").is_none());
        assert!(ExtendedBan::parse("$X:unknown").is_none());
    }

    #[test]
    fn test_extban_to_string() {
        let ban = ExtendedBan::Account("test".to_string());
        assert_eq!(ban.as_string(), "$a:test");

        let ban = ExtendedBan::Unregistered;
        assert_eq!(ban.as_string(), "$U");
    }

    #[test]
    fn test_extban_matches_account() {
        let ban = ExtendedBan::Account("spammer".to_string());
        let user = UserContext {
            nickname: "badguy".to_string(),
            username: "user".to_string(),
            hostname: "host.com".to_string(),
            realname: "Bad Guy".to_string(),
            account: Some("spammer".to_string()),
            server: "irc.example.com".to_string(),
            channels: vec![],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            ip_address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            is_registered: true,
        };

        assert!(ban.matches(&user));
    }

    #[test]
    fn test_extban_matches_unregistered() {
        let ban = ExtendedBan::Unregistered;
        let mut user = UserContext {
            nickname: "guest".to_string(),
            username: "user".to_string(),
            hostname: "host.com".to_string(),
            realname: "Guest".to_string(),
            account: None,
            server: "irc.example.com".to_string(),
            channels: vec![],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            ip_address: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            is_registered: false,
        };

        assert!(ban.matches(&user));

        user.is_registered = true;
        assert!(!ban.matches(&user));
    }

    #[test]
    fn test_wildcard_match() {
        assert!(wildcard_match("*bot*", "mybot123"));
        assert!(wildcard_match("*bot*", "123botxyz"));
        assert!(wildcard_match("user?", "user1"));
        assert!(wildcard_match("*.com", "example.com"));

        assert!(!wildcard_match("user?", "user12"));
        assert!(!wildcard_match("*.com", "example.org"));
    }

    #[test]
    fn test_xline_expiry() {
        let permanent = XLine::KLine {
            mask: "*@badhost.com".to_string(),
            reason: "Spam".to_string(),
            expires: None,
        };
        assert!(!permanent.is_expired());

        let expired = XLine::KLine {
            mask: "*@badhost.com".to_string(),
            reason: "Spam".to_string(),
            expires: Some(SystemTime::UNIX_EPOCH),
        };
        assert!(expired.is_expired());

        let future = XLine::KLine {
            mask: "*@badhost.com".to_string(),
            reason: "Spam".to_string(),
            expires: Some(SystemTime::now() + std::time::Duration::from_secs(3600)),
        };
        assert!(!future.is_expired());
    }

    #[test]
    fn test_xline_metadata() {
        let kline = XLine::KLine {
            mask: "*@badhost.com".to_string(),
            reason: "Spam detected".to_string(),
            expires: None,
        };

        assert_eq!(kline.line_type(), "K");
        assert_eq!(kline.pattern(), "*@badhost.com");
        assert_eq!(kline.reason(), "Spam detected");
    }
}
