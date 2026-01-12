//! METADATA subcommand types.
//!
//! METADATA allows clients and servers to get, set, and list metadata
//! associated with users and channels. This is an Ergo-defined extension.
//!
//! # Reference
//! - Ergo documentation: <https://ergo.chat/manual/commands/metadata>

use std::str::FromStr;

use crate::error::MessageParseError;

/// Subcommand for METADATA messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MetadataSubCommand {
    /// GET - Get metadata key for target
    GET,
    /// SET - Set metadata key for target
    SET,
    /// LIST - List all metadata for target
    LIST,
}

impl MetadataSubCommand {
    /// Get the string representation of this subcommand.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GET => "GET",
            Self::SET => "SET",
            Self::LIST => "LIST",
        }
    }
}

impl FromStr for MetadataSubCommand {
    type Err = MessageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Ok(Self::GET),
            "SET" => Ok(Self::SET),
            "LIST" => Ok(Self::LIST),
            _ => Err(MessageParseError::InvalidSubcommand {
                cmd: "METADATA",
                sub: s.to_owned(),
            }),
        }
    }
}

impl std::fmt::Display for MetadataSubCommand {
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
            "GET".parse::<MetadataSubCommand>().unwrap(),
            MetadataSubCommand::GET
        );
        assert_eq!(
            "get".parse::<MetadataSubCommand>().unwrap(),
            MetadataSubCommand::GET
        );
        assert_eq!(
            "SET".parse::<MetadataSubCommand>().unwrap(),
            MetadataSubCommand::SET
        );
        assert_eq!(
            "LIST".parse::<MetadataSubCommand>().unwrap(),
            MetadataSubCommand::LIST
        );
        assert!("INVALID".parse::<MetadataSubCommand>().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", MetadataSubCommand::GET), "GET");
        assert_eq!(format!("{}", MetadataSubCommand::SET), "SET");
        assert_eq!(format!("{}", MetadataSubCommand::LIST), "LIST");
    }

    #[test]
    fn test_as_str() {
        assert_eq!(MetadataSubCommand::GET.as_str(), "GET");
        assert_eq!(MetadataSubCommand::SET.as_str(), "SET");
        assert_eq!(MetadataSubCommand::LIST.as_str(), "LIST");
    }
}
