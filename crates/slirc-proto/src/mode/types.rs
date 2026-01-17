//! IRC mode types for users and channels.
//!
//! This module provides type-safe representations of IRC user modes and
//! channel modes as defined in RFC 2812 and extended by various IRC daemons.
//!
//! # Reference
//! - RFC 2812 Section 3.1.5 (User Modes)
//! - RFC 2812 Section 3.2.3 (Channel Modes)
//! - Modern IRC documentation: <https://modern.ircdocs.horse/>

use std::fmt;

/// Trait for mode types that can be applied to targets.
///
/// Implemented by [`UserMode`] and [`ChannelMode`].
pub trait ModeType: fmt::Display + fmt::Debug + Clone + PartialEq {
    /// Returns true if this mode takes an argument when set.
    fn takes_arg(&self) -> bool;

    /// Returns true if this is a Type A (list) mode that can be queried without an argument.
    ///
    /// Per RFC 2812 and Modern IRC docs, list modes (ban, exception, invite-exception)
    /// may be issued without an argument to query the current list contents.
    /// For example, `MODE #channel +b` queries the ban list.
    fn is_list_mode(&self) -> bool;

    /// Parse a mode character into its typed representation.
    fn from_char(c: char) -> Self;
}

/// User modes as defined in RFC 2812 and common extensions.
///
/// User modes modify the behavior of how the server and other users
/// interact with a particular user.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum UserMode {
    /// 'a' - User is away
    Away,
    /// 'i' - User is invisible (not shown in WHO/NAMES unless shared channel)
    Invisible,
    /// 'w' - User receives WALLOPS messages
    Wallops,
    /// 'r' - User is registered/identified with services
    ///
    /// Note: The meaning of `+r` varies by network:
    /// - On most modern networks: indicates the user is identified with NickServ
    /// - On some older networks: indicates a restricted connection
    Registered,
    /// 'R' - Only registered users can message
    RegisteredOnly,
    /// 'B' - User is marked as a bot
    Bot,
    /// 'S' - User is a network service
    Service,
    /// 'o' - User is an IRC operator
    Oper,
    /// 'O' - User is a local operator
    LocalOper,
    /// 's' - User receives server notices
    ServerNotices,
    /// 'x' - User's hostname is masked/cloaked
    MaskedHost,
    /// 'p' - Hide channels in WHOIS
    HideChannels,
    /// 'd' - User is deaf (doesn't receive channel messages)
    Deaf,
    /// 'g' - CallerID (whitelist-only private messaging)
    CallerId,
    /// 'N' - Network Administrator
    NetAdmin,
    /// Unknown mode character
    Unknown(char),
}

impl ModeType for UserMode {
    fn takes_arg(&self) -> bool {
        matches!(self, Self::ServerNotices)
    }

    fn is_list_mode(&self) -> bool {
        false // User modes are not list modes
    }

    fn from_char(c: char) -> Self {
        match c {
            'a' => Self::Away,
            'i' => Self::Invisible,
            'w' => Self::Wallops,
            'r' => Self::Registered,
            'R' => Self::RegisteredOnly,
            'B' => Self::Bot,
            'S' => Self::Service,
            'o' => Self::Oper,
            'O' => Self::LocalOper,
            's' => Self::ServerNotices,
            'x' => Self::MaskedHost,
            'p' => Self::HideChannels,
            'd' => Self::Deaf,
            'g' => Self::CallerId,
            'N' => Self::NetAdmin,
            _ => Self::Unknown(c),
        }
    }
}

impl fmt::Display for UserMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = match self {
            Self::Away => 'a',
            Self::Invisible => 'i',
            Self::Wallops => 'w',
            Self::Registered => 'r',
            Self::RegisteredOnly => 'R',
            Self::Bot => 'B',
            Self::Service => 'S',
            Self::Oper => 'o',
            Self::LocalOper => 'O',
            Self::ServerNotices => 's',
            Self::MaskedHost => 'x',
            Self::HideChannels => 'p',
            Self::Deaf => 'd',
            Self::CallerId => 'g',
            Self::NetAdmin => 'N',
            Self::Unknown(c) => *c,
        };
        write!(f, "{}", c)
    }
}

/// Channel modes as defined in RFC 2812 and common extensions.
///
/// Channel modes control channel behavior and user privileges within channels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ChannelMode {
    // === List modes (always take argument) ===
    /// 'b' - Ban mask
    Ban,
    /// 'e' - Ban exception mask
    Exception,
    /// 'I' - Invite exception mask
    InviteException,
    /// 'q' - Quiet mask (prevents talking but allows joining)
    Quiet,

    // === Modes that take argument when set ===
    /// 'l' - User limit (takes number when set)
    Limit,
    /// 'f' - Advanced flood protection
    Flood,
    /// 'F' - Channel forwarding (redirects joins to another channel)
    JoinForward,
    /// 'k' - Channel key
    Key,

    // === Modes without arguments ===
    /// 'i' - Invite only
    InviteOnly,
    /// 'm' - Moderated (only voiced+ can speak)
    Moderated,
    /// 'M' - Moderated for unregistered users (only registered can speak)
    ModeratedUnreg,
    /// 'U' - Op Moderated (messages from non-ops only go to ops)
    OpModerated,
    /// 'n' - No external messages
    NoExternalMessages,
    /// 'r' - Registered users only (on some servers)
    RegisteredOnly,
    /// 's' - Secret (hidden from LIST, WHO)
    Secret,
    /// 't' - Only ops can change topic
    ProtectedTopic,
    /// 'c' - Strip/block color codes
    NoColors,
    /// 'C' - No CTCP (except ACTION)
    NoCTCP,
    /// 'N' - No nick changes while in channel
    NoNickChange,
    /// 'K' - No KNOCK
    NoKnock,
    /// 'V' - No INVITE
    NoInvite,
    /// 'T' - No channel NOTICE
    NoChannelNotice,
    /// 'Q' - No kicks (peace mode)
    NoKick,
    /// 'u' - Auditorium (non-ops only see ops)
    Auditorium,
    /// 'P' - Permanent channel (persists with 0 users)
    Permanent,
    /// 'O' - Oper-only channel
    OperOnly,
    /// 'g' - Free INVITE (anyone can invite)
    FreeInvite,
    /// 'z' - TLS/SSL only channel
    TlsOnly,
    /// 'E' - Roleplay enabled (Ergo extension)
    Roleplay,
    /// 'D' - Delayed join
    DelayedJoin,
    /// 'S' - Strip color codes
    StripColors,
    /// 'B' - Anti-caps (block messages with too many caps)
    AntiCaps,
    /// 'L' - Redirect to another channel when limit (+l) exceeded
    Redirect,
    /// 'G' - Channel message filter/censor
    Censor,

    // === Prefix modes (grant channel privileges) ===
    /// 'q' - Channel founder (~) - note: conflicts with Quiet on some servers
    Founder,
    /// 'a' - Channel admin (&)
    Admin,
    /// 'o' - Channel operator (@)
    Oper,
    /// 'h' - Half-operator (%)
    Halfop,
    /// 'v' - Voice (+)
    Voice,

    /// Unknown mode character
    Unknown(char),
}

impl ModeType for ChannelMode {
    fn takes_arg(&self) -> bool {
        matches!(
            self,
            Self::Ban
                | Self::Exception
                | Self::InviteException
                | Self::Quiet
                | Self::Limit
                | Self::Key
                | Self::Flood
                | Self::JoinForward
                | Self::Founder
                | Self::Admin
                | Self::Oper
                | Self::Halfop
                | Self::Voice
                | Self::Redirect
        )
    }

    fn is_list_mode(&self) -> bool {
        // Type A modes per Modern IRC docs - can be queried without argument
        matches!(
            self,
            Self::Ban | Self::Exception | Self::InviteException | Self::Quiet
        )
    }

    fn from_char(c: char) -> Self {
        match c {
            'b' => Self::Ban,
            'e' => Self::Exception,
            'I' => Self::InviteException,
            'l' => Self::Limit,
            'k' => Self::Key,
            'f' => Self::Flood,
            'F' => Self::JoinForward,
            'i' => Self::InviteOnly,
            'm' => Self::Moderated,
            'M' => Self::ModeratedUnreg,
            'U' => Self::OpModerated,
            'n' => Self::NoExternalMessages,
            'r' => Self::RegisteredOnly,
            's' => Self::Secret,
            't' => Self::ProtectedTopic,
            'c' => Self::NoColors,
            'C' => Self::NoCTCP,
            'N' => Self::NoNickChange,
            'K' => Self::NoKnock,
            'V' => Self::NoInvite,
            'T' => Self::NoChannelNotice,
            'Q' => Self::NoKick,
            'u' => Self::Auditorium,
            'P' => Self::Permanent,
            'O' => Self::OperOnly,
            'g' => Self::FreeInvite,
            'z' => Self::TlsOnly,
            'E' => Self::Roleplay,
            'D' => Self::DelayedJoin,
            'S' => Self::StripColors,
            'B' => Self::AntiCaps,
            'L' => Self::Redirect,
            'G' => Self::Censor,
            'q' => Self::Quiet,
            // 'Q' => Self::Founder,
            'a' => Self::Admin,
            'o' => Self::Oper,
            'h' => Self::Halfop,
            'v' => Self::Voice,
            _ => Self::Unknown(c),
        }
    }
}

impl fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = match self {
            Self::Ban => 'b',
            Self::Exception => 'e',
            Self::InviteException => 'I',
            Self::Limit => 'l',
            Self::Key => 'k',
            Self::Flood => 'f',
            Self::JoinForward => 'F',
            Self::InviteOnly => 'i',
            Self::Moderated => 'm',
            Self::NoExternalMessages => 'n',
            Self::RegisteredOnly => 'r',
            Self::Secret => 's',
            Self::ProtectedTopic => 't',
            Self::NoColors => 'c',
            Self::NoCTCP => 'C',
            Self::NoNickChange => 'N',
            Self::NoKnock => 'K',
            Self::NoInvite => 'V',
            Self::NoChannelNotice => 'T',
            Self::NoKick => 'Q',
            Self::Permanent => 'P',
            Self::OperOnly => 'O',
            Self::FreeInvite => 'g',
            Self::TlsOnly => 'z',
            Self::Roleplay => 'E',
            Self::DelayedJoin => 'D',
            Self::StripColors => 'S',
            Self::AntiCaps => 'B',
            Self::Redirect => 'L',
            Self::Censor => 'G',
            Self::Quiet => 'q',
            Self::Founder => 'q',
            Self::Admin => 'a',
            Self::Oper => 'o',
            Self::Halfop => 'h',
            Self::Voice => 'v',
            Self::ModeratedUnreg => 'M',
            Self::OpModerated => 'U',
            Self::Auditorium => 'u',
            Self::Unknown(c) => *c,
        };
        write!(f, "{}", c)
    }
}

/// A mode change with its direction (+/-) and optional argument.
///
/// Represents a single mode change that can be applied to a target.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode<T: ModeType> {
    /// Mode is being added (+)
    Plus(T, Option<String>),
    /// Mode is being removed (-)
    Minus(T, Option<String>),
    /// Mode without prefix (used in query responses)
    NoPrefix(T),
}

impl<T: ModeType> Mode<T> {
    /// Create a mode addition with optional argument.
    pub fn plus(mode: T, arg: Option<&str>) -> Self {
        Self::Plus(mode, arg.map(String::from))
    }

    /// Create a mode removal with optional argument.
    pub fn minus(mode: T, arg: Option<&str>) -> Self {
        Self::Minus(mode, arg.map(String::from))
    }

    /// Create a mode without prefix.
    pub fn no_prefix(mode: T) -> Self {
        Self::NoPrefix(mode)
    }

    /// Get the mode flag string (e.g., "+o", "-v")
    pub fn flag(&self) -> String {
        match self {
            Self::Plus(m, _) => format!("+{}", m),
            Self::Minus(m, _) => format!("-{}", m),
            Self::NoPrefix(m) => m.to_string(),
        }
    }

    /// Get the argument if present.
    pub fn arg(&self) -> Option<&str> {
        match self {
            Self::Plus(_, arg) | Self::Minus(_, arg) => arg.as_deref(),
            Self::NoPrefix(_) => None,
        }
    }

    /// Get a reference to the inner mode type.
    pub fn mode(&self) -> &T {
        match self {
            Self::Plus(m, _) | Self::Minus(m, _) | Self::NoPrefix(m) => m,
        }
    }

    /// Returns true if this is adding a mode (+)
    pub fn is_plus(&self) -> bool {
        matches!(self, Self::Plus(..))
    }

    /// Returns true if this is removing a mode (-)
    pub fn is_minus(&self) -> bool {
        matches!(self, Self::Minus(..))
    }
}

impl<T: ModeType> fmt::Display for Mode<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plus(m, arg) => {
                write!(f, "+{}", m)?;
                if let Some(a) = arg {
                    write!(f, " {}", a)?;
                }
                Ok(())
            }
            Self::Minus(m, arg) => {
                write!(f, "-{}", m)?;
                if let Some(a) = arg {
                    write!(f, " {}", a)?;
                }
                Ok(())
            }
            Self::NoPrefix(m) => write!(f, "{}", m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_mode_display() {
        assert_eq!(format!("{}", UserMode::Invisible), "i");
        assert_eq!(format!("{}", UserMode::Oper), "o");
        assert_eq!(format!("{}", UserMode::Unknown('z')), "z");
    }

    #[test]
    fn test_user_mode_from_char() {
        assert_eq!(UserMode::from_char('i'), UserMode::Invisible);
        assert_eq!(UserMode::from_char('o'), UserMode::Oper);
        assert_eq!(UserMode::from_char('z'), UserMode::Unknown('z'));
    }

    #[test]
    fn test_channel_mode_display() {
        assert_eq!(format!("{}", ChannelMode::Oper), "o");
        assert_eq!(format!("{}", ChannelMode::Voice), "v");
        assert_eq!(format!("{}", ChannelMode::Ban), "b");
    }

    #[test]
    fn test_channel_mode_takes_arg() {
        assert!(ChannelMode::Ban.takes_arg());
        assert!(ChannelMode::Oper.takes_arg());
        assert!(ChannelMode::Key.takes_arg());
        assert!(!ChannelMode::Secret.takes_arg());
        assert!(!ChannelMode::InviteOnly.takes_arg());
    }

    #[test]
    fn test_mode_operations() {
        let mode = Mode::plus(ChannelMode::Oper, Some("nick"));
        assert_eq!(mode.flag(), "+o");
        assert_eq!(mode.arg(), Some("nick"));
        assert!(mode.is_plus());

        let mode = Mode::minus(ChannelMode::Voice, Some("user"));
        assert_eq!(mode.flag(), "-v");
        assert!(mode.is_minus());
    }

    #[test]
    fn test_mode_display() {
        let mode = Mode::plus(ChannelMode::Oper, Some("nick"));
        assert_eq!(format!("{}", mode), "+o nick");

        let mode = Mode::minus(UserMode::Invisible, None);
        assert_eq!(format!("{}", mode), "-i");
    }
}
