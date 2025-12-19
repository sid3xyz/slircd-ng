//! Extended Bans for channel-level moderation.
//!
//! Provides pattern matching beyond nick!user@host for channel bans (+b).
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
//!
//! # Note on X-Lines (K/G/Z/D-Lines)
//!
//! Server-level bans are handled by [`crate::security::BanCache`] which loads
//! from the database models in [`crate::db::BanRepository`]. This module only handles
//! extended ban patterns for channel mode +b.

use slirc_proto::wildcard_match;

/// Extended Ban types for advanced pattern matching.
///
/// Used for channel bans (+b) to match on user attributes beyond nick!user@host.
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

}

/// User context for evaluating extended bans.
///
/// Contains all user attributes that extended bans can match against.
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
    /// Whether the user has identified to an account.
    pub is_registered: bool,
    /// Whether the user is connected via TLS.
    pub is_tls: bool,
}

/// Parameters for creating a UserContext during registration.
#[derive(Debug, Clone)]
pub struct RegistrationParams {
    pub hostname: String,
    pub nickname: String,
    pub username: String,
    pub realname: String,
    pub server: String,
    pub account: Option<String>,
    pub is_tls: bool,
    pub is_oper: bool,
    pub oper_type: Option<String>,
}

impl UserContext {
    /// Create a context for registration-time checks (after NICK/USER, before welcome).
    pub fn for_registration(params: RegistrationParams) -> Self {
        let RegistrationParams {
            hostname,
            nickname,
            username,
            realname,
            server,
            account,
            is_tls,
            is_oper,
            oper_type,
        } = params;

        Self {
            nickname,
            username,
            hostname,
            realname,
            account: account.clone(),
            server,
            channels: Vec::new(),
            is_oper,
            oper_type,
            certificate_fp: None,
            sasl_mechanism: None,
            is_registered: account.is_some(),
            is_tls,
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
            is_tls: false,
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
}
