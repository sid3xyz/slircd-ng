//! PING and PONG command implementation
//!
//! RFC 2812 Section 3.7.2: Ping message
//!
//! The PING command is used to test the presence of an active client or
//! server at the other end of the connection.
//!
//! Syntax: PING <token>
//! Syntax: PONG <token>

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::Result;

/// PING command - test connection liveness
#[derive(Debug, Clone)]
pub struct PingCommand {
    token: String,
}

impl Command for PingCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        let token = if parts.len() > 1 {
            parts[1].trim_start_matches(':').to_string()
        } else {
            String::from("server")
        };

        Ok(Box::new(PingCommand { token }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        // Respond with PONG
        ctx.send_raw(format!("PONG :{}",  &self.token))?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "PING"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::None // Can ping before registration
    }
}

/// PONG command - response to PING
#[derive(Debug, Clone)]
pub struct PongCommand {
    token: String,
}

impl Command for PongCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        let token = if parts.len() > 1 {
            parts[1].trim_start_matches(':').to_string()
        } else {
            String::new()
        };

        Ok(Box::new(PongCommand { token }))
    }

    fn execute(&self, _ctx: &mut ExecutionContext) -> Result<()> {
        // PONG received - just update activity timestamp
        // TODO: Update last_activity in client state
        tracing::debug!(token = %self.token, "pong received");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "PONG"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ping() {
        let result = PingCommand::parse(&["PING", "test123"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_ping_no_token() {
        let result = PingCommand::parse(&["PING"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_pong() {
        let result = PongCommand::parse(&["PONG", ":test123"]);
        assert!(result.is_ok());
    }
}
