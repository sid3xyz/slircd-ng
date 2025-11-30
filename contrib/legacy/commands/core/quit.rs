//! QUIT command implementation
//!
//! RFC 2812 Section 3.1.7: Quit
//!
//! A client session is terminated with a quit message.
//!
//! Syntax: QUIT [:<message>]

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::Result;

/// QUIT command - disconnect from server
#[derive(Debug, Clone)]
pub struct QuitCommand {
    reason: String,
}

impl Command for QuitCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        let reason = if parts.len() > 1 {
            parts[1..].join(" ").trim_start_matches(':').to_string()
        } else {
            String::from("Client quit")
        };

        Ok(Box::new(QuitCommand { reason }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        // Send ERROR message
        ctx.send_raw(format!("ERROR :Closing Link: {}", self.reason))?;
        
        // Disconnect client
        ctx.disconnect(&self.reason)?;
        
        tracing::info!(client_id = ctx.client_id, reason = %self.reason, "client quit");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "QUIT"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::None // Can quit anytime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quit_with_reason() {
        let result = QuitCommand::parse(&["QUIT", ":Goodbye", "world"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_quit_no_reason() {
        let result = QuitCommand::parse(&["QUIT"]);
        assert!(result.is_ok());
    }
}
