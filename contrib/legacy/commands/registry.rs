//! Command registry - parse dispatcher and command registration
//!
//! Central registry for all IRC commands:
//! - Parses raw IRC lines into command objects
//! - Dispatches to appropriate command parser
//! - Handles unknown commands gracefully

use anyhow::{Result, Context as AnyhowContext};
use std::collections::HashMap;
use std::sync::LazyLock;
use super::r#trait::Command;

/// Command parser function type
type ParserFn = fn(&[&str]) -> Result<Box<dyn Command>>;

/// Registry of IRC commands
pub struct CommandRegistry {
    parsers: HashMap<String, ParserFn>,
}

impl CommandRegistry {
    /// Create empty registry
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
        }
    }

    /// Register command parser
    /// 
    /// # Example
    /// ```ignore
    /// registry.register("NICK", NickCommand::parse);
    /// ```
    pub fn register(&mut self, name: &str, parser: ParserFn) {
        self.parsers.insert(name.to_uppercase(), parser);
    }

    /// Parse IRC line into command
    /// 
    /// # Arguments
    /// * `line` - Raw IRC line (may include CRLF)
    /// 
    /// # Returns
    /// - Ok(Some(command)) - Successfully parsed known command
    /// - Ok(None) - Empty line or whitespace only
    /// - Err(_) - Parse error or unknown command
    pub fn parse(&self, line: &str) -> Result<Option<Box<dyn Command>>> {
        // Normalize: strip non-printable chars
        let clean = line
            .trim()
            .chars()
            .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
            .collect::<String>();

        let parts: Vec<&str> = clean.split_whitespace().collect();

        if parts.is_empty() {
            return Ok(None);
        }

        let command = parts[0].to_uppercase();

        // Lookup parser
        let parser = self.parsers.get(&command)
            .context(format!("Unknown command: {}", command))?;

        // Parse command
        let cmd = parser(&parts)
            .context(format!("Failed to parse {} command", command))?;

        Ok(Some(cmd))
    }
}

/// Global command registry (lazy-initialized)
pub static REGISTRY: LazyLock<CommandRegistry> = LazyLock::new(|| {
    let mut reg = CommandRegistry::new();
    
    // Register core commands
    reg.register("NICK", crate::commands::core::nick::NickCommand::parse);
    reg.register("USER", crate::commands::core::user::UserCommand::parse);
    reg.register("MODE", crate::commands::core::mode::ModeCommand::parse);
    reg.register("PRIVMSG", crate::commands::core::privmsg::PrivmsgCommand::parse);
    reg.register("NOTICE", crate::commands::core::privmsg::PrivmsgCommand::parse);
    reg.register("PING", crate::commands::core::ping::PingCommand::parse);
    reg.register("PONG", crate::commands::core::ping::PongCommand::parse);
    reg.register("QUIT", crate::commands::core::quit::QuitCommand::parse);
    
    reg
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_line() {
        let reg = CommandRegistry::new();
        let result = reg.parse("   \r\n");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_unknown_command() {
        let reg = CommandRegistry::new();
        let result = reg.parse("UNKNOWN test\r\n");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown command"));
    }
}
