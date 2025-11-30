//! PRIVMSG and NOTICE command implementation
//!
//! RFC 2812 Section 3.3.1: Private messages
//!
//! Used to send private messages between users, as well as to send
//! messages to channels.
//!
//! Syntax: PRIVMSG <target> :<text>
//! Syntax: NOTICE <target> :<text>
//!
//! Numeric replies:
//! - 401 ERR_NOSUCHNICK - No such nick/channel
//! - 411 ERR_NORECIPIENT - No recipient given
//! - 412 ERR_NOTEXTTOSEND - No text to send
//! - 404 ERR_CANNOTSENDTOCHAN - Cannot send to channel

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::{Result, bail};

/// PRIVMSG/NOTICE command - send messages to users or channels
#[derive(Debug, Clone)]
pub struct PrivmsgCommand {
    target: String,
    text: String,
    is_notice: bool,
}

impl Command for PrivmsgCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        let is_notice = parts[0].eq_ignore_ascii_case("NOTICE");
        
        if parts.len() < 2 {
            let code = if is_notice { "411" } else { "411" };
            bail!("{} :No recipient given ({})", code, parts[0]);
        }

        if parts.len() < 3 {
            bail!("412 :No text to send");
        }

        let target = parts[1].to_string();
        let text = parts[2..].join(" ").trim_start_matches(':').to_string();

        if text.is_empty() {
            bail!("412 :No text to send");
        }

        Ok(Box::new(PrivmsgCommand { target, text, is_notice }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        let sender_nick = ctx.client_state.nickname.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No nickname set"))?;

        // Check if target is a channel or user
        if self.target.starts_with('#') || self.target.starts_with('&') {
            // Channel message - TODO: implement when channels are in ExecutionContext
            if !self.is_notice {
                ctx.send_numeric(404, &[&self.target], "Cannot send to channel (not implemented yet)")?;
            }
        } else {
            // Private message to user
            let server_state = ctx.server_state.blocking_read();
            
            // Find target client
            let target_client = server_state.clients.iter()
                .find(|(_, state)| {
                    state.nickname.as_ref().map(|n| n.eq_ignore_ascii_case(&self.target)).unwrap_or(false)
                });

            match target_client {
                Some((target_id, _)) => {
                    let target_id = *target_id;
                    drop(server_state); // Release lock
                    
                    // Send message to target
                    // TODO: Need access to sessions HashMap to send to target
                    // For now, just log success
                    tracing::debug!(
                        from = %sender_nick, 
                        to = %self.target, 
                        text = %self.text,
                        "privmsg sent"
                    );
                }
                None => {
                    drop(server_state);
                    if !self.is_notice {
                        ctx.send_numeric(401, &[&self.target], "No such nick/channel")?;
                    }
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        if self.is_notice {
            "NOTICE"
        } else {
            "PRIVMSG"
        }
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_privmsg() {
        let result = PrivmsgCommand::parse(&["PRIVMSG", "bob", ":Hello", "world"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_notice() {
        let result = PrivmsgCommand::parse(&["NOTICE", "alice", ":Test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_no_recipient() {
        let result = PrivmsgCommand::parse(&["PRIVMSG"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("411"));
    }

    #[test]
    fn test_parse_no_text() {
        let result = PrivmsgCommand::parse(&["PRIVMSG", "bob"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("412"));
    }
}
