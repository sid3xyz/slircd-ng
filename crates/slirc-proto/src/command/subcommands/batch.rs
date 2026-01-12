//! BATCH subcommand types for IRCv3 message batching.
//!
//! # Reference
//! - IRCv3 batch specification: <https://ircv3.net/specs/extensions/batch>

use std::str::FromStr;

use crate::error::MessageParseError;

/// Subcommand/type for BATCH messages.
///
/// BATCH is used to group related messages together.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum BatchSubCommand {
    /// Network split indication
    NETSPLIT,
    /// Network rejoin indication
    NETJOIN,
    /// Custom/unknown batch type
    CUSTOM(String),
}

impl BatchSubCommand {
    /// Get the string representation of this subcommand.
    pub fn as_str(&self) -> &str {
        match self {
            Self::NETSPLIT => "NETSPLIT",
            Self::NETJOIN => "NETJOIN",
            Self::CUSTOM(s) => s,
        }
    }

    /// Alias for backward compatibility
    #[inline]
    pub fn to_str(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for BatchSubCommand {
    type Err = MessageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "NETSPLIT" => Ok(Self::NETSPLIT),
            "NETJOIN" => Ok(Self::NETJOIN),
            _ => Ok(Self::CUSTOM(upper)),
        }
    }
}

impl std::fmt::Display for BatchSubCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(
            "NETSPLIT".parse::<BatchSubCommand>().unwrap(),
            BatchSubCommand::NETSPLIT
        );
        assert_eq!(
            "netjoin".parse::<BatchSubCommand>().unwrap(),
            BatchSubCommand::NETJOIN
        );
        assert_eq!(
            "chathistory".parse::<BatchSubCommand>().unwrap(),
            BatchSubCommand::CUSTOM("CHATHISTORY".to_string())
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", BatchSubCommand::NETSPLIT), "NETSPLIT");
        assert_eq!(
            format!("{}", BatchSubCommand::CUSTOM("TEST".to_string())),
            "TEST"
        );
    }
}
