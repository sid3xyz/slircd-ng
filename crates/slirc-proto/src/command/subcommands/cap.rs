//! CAP subcommand types for IRCv3 capability negotiation.
//!
//! # Reference
//! - IRCv3 capability negotiation: <https://ircv3.net/specs/extensions/capability-negotiation>

use std::str::FromStr;

use crate::error::MessageParseError;

/// Subcommand for CAP (capability negotiation) messages.
///
/// CAP is used to negotiate IRCv3 capabilities between client and server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CapSubCommand {
    /// LS - List available capabilities
    LS,
    /// LIST - List currently enabled capabilities
    LIST,
    /// REQ - Request capabilities
    REQ,
    /// ACK - Server acknowledged capabilities
    ACK,
    /// NAK - Server rejected capabilities
    NAK,
    /// END - End capability negotiation
    END,
    /// NEW - Server advertising new capabilities (cap-notify)
    NEW,
    /// DEL - Server removing capabilities (cap-notify)
    DEL,
}

impl CapSubCommand {
    /// Get the string representation of this subcommand.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LS => "LS",
            Self::LIST => "LIST",
            Self::REQ => "REQ",
            Self::ACK => "ACK",
            Self::NAK => "NAK",
            Self::END => "END",
            Self::NEW => "NEW",
            Self::DEL => "DEL",
        }
    }

    /// Alias for backward compatibility
    #[inline]
    pub fn to_str(&self) -> &'static str {
        self.as_str()
    }
}

impl FromStr for CapSubCommand {
    type Err = MessageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "LS" => Ok(Self::LS),
            "LIST" => Ok(Self::LIST),
            "REQ" => Ok(Self::REQ),
            "ACK" => Ok(Self::ACK),
            "NAK" => Ok(Self::NAK),
            "END" => Ok(Self::END),
            "NEW" => Ok(Self::NEW),
            "DEL" => Ok(Self::DEL),
            _ => Err(MessageParseError::InvalidSubcommand {
                cmd: "CAP",
                sub: s.to_owned(),
            }),
        }
    }
}

impl std::fmt::Display for CapSubCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!("LS".parse::<CapSubCommand>().unwrap(), CapSubCommand::LS);
        assert_eq!("ls".parse::<CapSubCommand>().unwrap(), CapSubCommand::LS);
        assert_eq!("req".parse::<CapSubCommand>().unwrap(), CapSubCommand::REQ);
        assert!("INVALID".parse::<CapSubCommand>().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", CapSubCommand::LS), "LS");
        assert_eq!(format!("{}", CapSubCommand::REQ), "REQ");
    }
}
