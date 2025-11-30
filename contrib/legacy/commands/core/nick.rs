//! NICK command implementation
//!
//! RFC 2812 Section 3.1.2: Nick message
//! 
//! Used to give user a nickname or change the existing one.
//!
//! Syntax: NICK <nickname>
//!
//! Numeric replies:
//! - 431 ERR_NONICKNAMEGIVEN - No nickname given
//! - 432 ERR_ERRONEUSNICKNAME - Invalid nickname format
//! - 433 ERR_NICKNAMEINUSE - Nickname is already in use
//! - 001 RPL_WELCOME - Registration complete (if NICK+USER both received)

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::{Result, bail, Context as AnyhowContext};

/// NICK command - set or change nickname
#[derive(Debug, Clone)]
pub struct NickCommand {
    nickname: String,
}

impl Command for NickCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        if parts.len() < 2 {
            bail!("431 :No nickname given");
        }

        let nickname = parts[1].to_string();

        // RFC 2812: nickname validation
        // - First char: letter or special ({|}[]^_-)
        // - Subsequent: letter, digit, special, or hyphen
        // - Max 9 chars (traditional), 30 chars (modern)
        if nickname.is_empty() || nickname.len() > 30 {
            bail!("432 {} :Erroneous nickname", nickname);
        }

        let first = nickname.chars().next().unwrap();
        if !first.is_ascii_alphabetic() && !matches!(first, '{' | '}' | '[' | ']' | '\\' | '|' | '^' | '_' | '-') {
            bail!("432 {} :Erroneous nickname", nickname);
        }

        Ok(Box::new(NickCommand { nickname }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        // Check if nickname is already in use
        let server_state = ctx.server_state.blocking_read();
        
        for (client_id, state) in server_state.clients.iter() {
            if *client_id != ctx.client_id {
                if let Some(ref nick) = state.nickname {
                    if nick.eq_ignore_ascii_case(&self.nickname) {
                        drop(server_state); // Release lock before sending
                        return ctx.send_numeric(433, &[&self.nickname], "Nickname is already in use");
                    }
                }
            }
        }
        
        drop(server_state); // Release read lock before write

        // Set nickname
        let mut server_state = ctx.server_state.blocking_write();
        if let Some(client_state) = server_state.clients.get_mut(&ctx.client_id) {
            let old_nick = client_state.nickname.clone();
            client_state.nickname = Some(self.nickname.clone());

            // Check if registration is now complete
            let has_user = client_state.username.is_some();
            let was_registered = client_state.registered;
            
            if has_user && !was_registered {
                client_state.registered = true;
                drop(server_state); // Release before sending
                
                // Send welcome messages
                ctx.send_numeric(001, &[], &format!("Welcome to the Internet Relay Network {}", self.nickname))?;
                ctx.send_numeric(002, &[], &format!("Your host is slircd, running version slircd-3.0"))?;
                ctx.send_numeric(004, &[], "slircd slircd-3.0 o o")?;
                
                tracing::info!(client_id = ctx.client_id, nickname = %self.nickname, "registration complete");
            } else if old_nick.is_some() {
                drop(server_state);
                // Nickname change (already registered)
                // TODO: Broadcast NICK change to visible clients
                tracing::debug!(client_id = ctx.client_id, old = ?old_nick, new = %self.nickname, "nickname changed");
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "NICK"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::None // Can be used during registration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_nick() {
        let result = NickCommand::parse(&["NICK", "alice"]);
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd.name(), "NICK");
    }

    #[test]
    fn test_parse_no_nickname() {
        let result = NickCommand::parse(&["NICK"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("431"));
    }

    #[test]
    fn test_parse_invalid_nickname() {
        let result = NickCommand::parse(&["NICK", "123invalid"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("432"));
    }

    #[test]
    fn test_parse_too_long() {
        let result = NickCommand::parse(&["NICK", "a".repeat(31).as_str()]);
        assert!(result.is_err());
    }
}
