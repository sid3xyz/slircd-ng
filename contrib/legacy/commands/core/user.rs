//! USER command implementation
//!
//! RFC 2812 Section 3.1.3: User message
//!
//! The USER command is used at the beginning of connection to specify
//! the username, hostname and realname of a new user.
//!
//! Syntax: USER <username> <mode> <unused> :<realname>
//!
//! Numeric replies:
//! - 461 ERR_NEEDMOREPARAMS - Not enough parameters
//! - 462 ERR_ALREADYREGISTRED - Already registered
//! - 001 RPL_WELCOME - Registration complete (if NICK also received)

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::{Result, bail};

/// USER command - set username and realname during registration
#[derive(Debug, Clone)]
pub struct UserCommand {
    username: String,
    mode: u8,
    realname: String,
}

impl Command for UserCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        if parts.len() < 5 {
            bail!("461 USER :Not enough parameters");
        }

        let username = parts[1].to_string();
        let mode = parts[2].parse::<u8>().unwrap_or(0);
        // parts[3] is unused per RFC
        let realname = parts[4..].join(" ").trim_start_matches(':').to_string();

        Ok(Box::new(UserCommand { username, mode, realname }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        // Check if already registered
        if ctx.client_state.registered {
            return ctx.send_numeric(462, &[], "You may not reregister");
        }

        // Set username and realname
        let mut server_state = ctx.server_state.blocking_write();
        if let Some(client_state) = server_state.clients.get_mut(&ctx.client_id) {
            client_state.username = Some(self.username.clone());
            client_state.realname = Some(self.realname.clone());

            // Check if registration is complete (NICK also received)
            if client_state.is_ready_to_register() {
                client_state.registered = true;
                let nickname = client_state.nickname.clone().unwrap();
                drop(server_state); // Release lock before sending

                // Send welcome messages
                ctx.send_numeric(001, &[], &format!("Welcome to the Internet Relay Network {}", nickname))?;
                ctx.send_numeric(002, &[], "Your host is slircd, running version slircd-3.0")?;
                ctx.send_numeric(004, &[], "slircd slircd-3.0 o o")?;

                tracing::info!(client_id = ctx.client_id, nickname = %nickname, "registration complete");
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "USER"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::None // Can be used during registration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid() {
        let result = UserCommand::parse(&["USER", "alice", "0", "*", ":Alice Smith"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_not_enough_params() {
        let result = UserCommand::parse(&["USER", "alice"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("461"));
    }

    #[test]
    fn test_parse_multiword_realname() {
        let result = UserCommand::parse(&["USER", "bob", "8", "*", ":Bob", "The", "Builder"]);
        assert!(result.is_ok());
    }
}
