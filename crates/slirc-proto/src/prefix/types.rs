//! IRC message prefix types.
//!
//! An IRC message prefix identifies the origin of a message. It can be either
//! a server name or a user's nick!user@host mask.
//!
//! # Reference
//! - RFC 2812 Section 2.3.1: Message format

use std::str::FromStr;

use crate::error::MessageParseError;

/// IRC message prefix - identifies the origin of a message.
///
/// A prefix can be either a server name (containing a dot) or a user's
/// nick!user@host identifier.
#[derive(Clone, Eq, PartialEq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Prefix {
    /// Server name (e.g., "irc.example.com")
    ServerName(String),
    /// User prefix: (nickname, username, hostname)
    Nickname(String, String, String),
}

impl Prefix {
    /// Parse a prefix string into a Prefix.
    ///
    /// This is a lenient parser that does not validate the components.
    pub fn new_from_str(s: &str) -> Self {
        #[derive(Copy, Clone, Eq, PartialEq)]
        enum Part {
            Name,
            User,
            Host,
        }

        let mut name = String::new();
        let mut user = String::new();
        let mut host = String::new();
        let mut part = Part::Name;
        let mut is_server = false;

        for c in s.chars() {
            // A dot in the name part (before ! or @) suggests server name
            if c == '.' && part == Part::Name {
                is_server = true;
            }

            match c {
                '!' if part == Part::Name => {
                    is_server = false;
                    part = Part::User;
                }
                '@' if part != Part::Host => {
                    is_server = false;
                    part = Part::Host;
                }
                _ => {
                    match part {
                        Part::Name => &mut name,
                        Part::User => &mut user,
                        Part::Host => &mut host,
                    }
                    .push(c);
                }
            }
        }

        if is_server {
            Prefix::ServerName(name)
        } else {
            Prefix::Nickname(name, user, host)
        }
    }

    /// Create a new user prefix from nick, user, and host components.
    ///
    /// This is a shorthand for `Prefix::Nickname(nick.into(), user.into(), host.into())`.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::Prefix;
    ///
    /// let prefix = Prefix::new("nick", "user", "host.example.com");
    /// assert_eq!(prefix.nick(), Some("nick"));
    /// assert_eq!(prefix.user(), Some("user"));
    /// assert_eq!(prefix.host(), Some("host.example.com"));
    /// ```
    pub fn new(nick: impl Into<String>, user: impl Into<String>, host: impl Into<String>) -> Self {
        Prefix::Nickname(nick.into(), user.into(), host.into())
    }

    /// Parse with validation, returning an error for invalid prefixes.
    pub fn try_from_str(s: &str) -> Result<Self, MessageParseError> {
        if validate_prefix(s) {
            Ok(Self::new_from_str(s))
        } else {
            Err(MessageParseError::InvalidPrefix(s.to_owned()))
        }
    }

    /// Get the nickname if this is a user prefix.
    pub fn nick(&self) -> Option<&str> {
        match self {
            Prefix::Nickname(nick, _, _) if !nick.is_empty() => Some(nick),
            _ => None,
        }
    }

    /// Get the username if this is a user prefix.
    pub fn user(&self) -> Option<&str> {
        match self {
            Prefix::Nickname(_, user, _) if !user.is_empty() => Some(user),
            _ => None,
        }
    }

    /// Get the hostname.
    pub fn host(&self) -> Option<&str> {
        match self {
            Prefix::ServerName(name) => Some(name),
            Prefix::Nickname(_, _, host) if !host.is_empty() => Some(host),
            _ => None,
        }
    }
}

impl FromStr for Prefix {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Prefix::new_from_str(s))
    }
}

impl From<&str> for Prefix {
    fn from(s: &str) -> Self {
        Prefix::new_from_str(s)
    }
}

/// Check if a prefix string is valid.
///
/// A valid prefix:
/// - Is non-empty
/// - Contains no NUL, control characters, or spaces
/// - If it has @ or !, follows the nick!user@host format
#[cfg(test)]
pub(crate) fn is_valid_prefix_str(s: &str) -> bool {
    validate_prefix(s)
}

fn validate_prefix(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // No NUL, control characters, or spaces
    if s.chars().any(|c| c == '\0' || c.is_control() || c == ' ') {
        return false;
    }

    // If no ! or @, it's just a name - valid
    if !s.contains('!') && !s.contains('@') {
        return true;
    }

    // If has @ or !, must follow nick!user@host format
    let at_pos = match s.find('@') {
        Some(i) => i,
        None => return false, // Has ! but no @ - invalid
    };

    let before_at = &s[..at_pos];
    let host = &s[at_pos + 1..];

    if host.is_empty() {
        return false;
    }

    let (nick, user) = match before_at.find('!') {
        Some(bang) => (&before_at[..bang], &before_at[bang + 1..]),
        None => (before_at, ""),
    };

    // Nick is required
    if nick.is_empty() {
        return false;
    }

    // Validate nickname format
    if !validate_nickname(nick) {
        return false;
    }

    // User can't have @ or control chars
    if !user.is_empty() && user.chars().any(|c| c == '@' || c == ' ' || c.is_control()) {
        return false;
    }

    // Host can't have spaces or control chars
    if host.chars().any(|c| c == ' ' || c.is_control()) {
        return false;
    }

    true
}

fn validate_nickname(nick: &str) -> bool {
    let mut chars = nick.chars();

    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };

    // Special characters allowed in nicknames: [ ] \ ` _ ^ { | }
    let is_special = |c: char| {
        let code = c as u32;
        (0x5B..=0x60).contains(&code) || (0x7B..=0x7D).contains(&code)
    };

    // First char: letter or special
    if !(first.is_ascii_alphabetic() || is_special(first)) {
        return false;
    }

    // Rest: letter, digit, special, or hyphen
    for c in chars {
        if !(c.is_ascii_alphanumeric() || is_special(c) || c == '-') {
            return false;
        }
    }

    // Reasonable length limit
    nick.len() <= 50
}

/// A borrowed reference to a parsed prefix.
///
/// Used for zero-copy parsing of IRC messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixRef<'a> {
    /// Nickname (or server name if host contains a dot)
    pub nick: Option<&'a str>,
    /// Username (ident)
    pub user: Option<&'a str>,
    /// Hostname
    pub host: Option<&'a str>,
    /// Original raw prefix string
    pub raw: &'a str,
}

impl<'a> PrefixRef<'a> {
    /// Parse a prefix string into components without allocation.
    pub fn parse(s: &'a str) -> Self {
        // Look for @ first (nick!user@host format)
        if let Some(at_pos) = s.find('@') {
            let before = &s[..at_pos];
            let host = &s[at_pos + 1..];

            let (nick, user) = match before.find('!') {
                Some(bang) => {
                    let n = &before[..bang];
                    let u = &before[bang + 1..];
                    (
                        if n.is_empty() { None } else { Some(n) },
                        if u.is_empty() { None } else { Some(u) },
                    )
                }
                None => (
                    if before.is_empty() {
                        None
                    } else {
                        Some(before)
                    },
                    None,
                ),
            };

            Self {
                nick,
                user,
                host: Some(host),
                raw: s,
            }
        } else if let Some(bang) = s.find('!') {
            // nick!user without @host
            let nick = &s[..bang];
            let user = &s[bang + 1..];

            Self {
                nick: if nick.is_empty() { None } else { Some(nick) },
                user: if user.is_empty() { None } else { Some(user) },
                host: None,
                raw: s,
            }
        } else if s.contains('.') {
            // Contains dot, likely server name
            Self {
                nick: None,
                user: None,
                host: Some(s),
                raw: s,
            }
        } else {
            // Just a nick
            Self {
                nick: Some(s),
                user: None,
                host: None,
                raw: s,
            }
        }
    }

    /// Check if this prefix looks like a server name.
    pub fn is_server(&self) -> bool {
        self.nick.is_none() && self.user.is_none() && self.host.is_some()
    }

    /// Get the nickname if this is a user prefix.
    ///
    /// Returns `None` if this is a server name prefix.
    #[inline]
    pub fn nickname(&self) -> Option<&'a str> {
        self.nick
    }

    /// Convert to an owned Prefix.
    pub fn to_owned(&self) -> Prefix {
        if self.is_server() {
            Prefix::ServerName(self.host.unwrap_or("").to_string())
        } else {
            Prefix::Nickname(
                self.nick.unwrap_or("").to_string(),
                self.user.unwrap_or("").to_string(),
                self.host.unwrap_or("").to_string(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server_name() {
        let p = Prefix::new_from_str("irc.example.com");
        assert_eq!(p, Prefix::ServerName("irc.example.com".into()));
    }

    #[test]
    fn test_parse_nick_user_host() {
        let p = Prefix::new_from_str("nick!user@host.com");
        assert_eq!(
            p,
            Prefix::Nickname("nick".into(), "user".into(), "host.com".into())
        );
    }

    #[test]
    fn test_parse_nick_only() {
        let p = Prefix::new_from_str("nickname");
        assert_eq!(p, Prefix::Nickname("nickname".into(), "".into(), "".into()));
    }

    #[test]
    fn test_prefix_ref_parse() {
        let p = PrefixRef::parse("nick!user@host.com");
        assert_eq!(p.nick, Some("nick"));
        assert_eq!(p.user, Some("user"));
        assert_eq!(p.host, Some("host.com"));
    }

    #[test]
    fn test_prefix_ref_server() {
        let p = PrefixRef::parse("irc.example.com");
        assert!(p.is_server());
        assert_eq!(p.host, Some("irc.example.com"));
    }

    #[test]
    fn test_valid_prefix() {
        assert!(is_valid_prefix_str("nick!user@host"));
        assert!(is_valid_prefix_str("irc.example.com"));
        assert!(is_valid_prefix_str("nickname"));
        assert!(!is_valid_prefix_str(""));
        assert!(!is_valid_prefix_str("nick with space"));
    }

    #[test]
    fn test_prefix_accessors() {
        let p = Prefix::Nickname("nick".into(), "user".into(), "host".into());
        assert_eq!(p.nick(), Some("nick"));
        assert_eq!(p.user(), Some("user"));
        assert_eq!(p.host(), Some("host"));

        let s = Prefix::ServerName("irc.test.com".into());
        assert_eq!(s.nick(), None);
        assert_eq!(s.host(), Some("irc.test.com"));
    }
}
