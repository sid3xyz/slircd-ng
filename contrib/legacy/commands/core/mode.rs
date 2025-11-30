//! MODE command implementation
//!
//! RFC 2812 Section 3.1.5: User mode message
//! RFC 2812 Section 3.2.3: Channel mode message
//!
//! User MODE: MODE <nickname> [+|-]<modes>
//! Channel MODE: MODE <channel> [+|-]<modes> [params]
//!
//! User modes (RFC 2812):
//! - i: Invisible (not shown in WHO/WHOIS unless querier shares channel)
//! - w: Wallops (receive WALLOPS messages)
//! - o: IRC operator
//! - s: Server notices
//!
//! Numeric replies:
//! - 221 RPL_UMODEIS - Current user modes
//! - 501 ERR_UMODEUNKNOWNFLAG - Unknown user mode flag
//! - 502 ERR_USERSDONTMATCH - Cannot change mode for other users

use crate::commands::r#trait::{Command, RegistrationLevel};
use crate::commands::context::ExecutionContext;
use anyhow::{Result, bail};

/// MODE command - query or set user/channel modes
#[derive(Debug, Clone)]
pub struct ModeCommand {
    target: String,
    modes: Option<String>,  // None = query, Some = set
    params: Vec<String>,
}

impl Command for ModeCommand {
    fn parse(parts: &[&str]) -> Result<Box<dyn Command>> {
        if parts.len() < 2 {
            bail!("461 MODE :Not enough parameters");
        }

        let target = parts[1].to_string();
        let modes = if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None  // Query mode
        };
        let params = parts[3..].iter().map(|s| s.to_string()).collect();

        Ok(Box::new(ModeCommand { target, modes, params }))
    }

    fn execute(&self, ctx: &mut ExecutionContext) -> Result<()> {
        // Determine if target is channel or user
        let is_channel = self.target.starts_with('#') || self.target.starts_with('&');

        if is_channel {
            self.execute_channel_mode(ctx)
        } else {
            self.execute_user_mode(ctx)
        }
    }

    fn name(&self) -> &'static str {
        "MODE"
    }

    fn min_registration(&self) -> RegistrationLevel {
        RegistrationLevel::Full
    }
}

impl ModeCommand {
    /// Execute user MODE command
    fn execute_user_mode(&self, ctx: &mut ExecutionContext) -> Result<()> {
        let my_nick = ctx.client_state.nickname.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Client has no nickname"))?;

        // RFC 2812: Users can only change their own modes
        if !self.target.eq_ignore_ascii_case(my_nick) {
            return ctx.send_numeric(502, &[], "Cannot change mode for other users");
        }

        match &self.modes {
            None => {
                // Query current modes
                let modes_str = format!("+{}", ctx.client_state.modes.iter().collect::<String>());
                ctx.send_numeric(221, &[&modes_str], "User modes")
            }
            Some(mode_str) => {
                // Set modes
                self.apply_user_modes(ctx, mode_str)
            }
        }
    }

    /// Apply user mode changes
    fn apply_user_modes(&self, ctx: &mut ExecutionContext, mode_str: &str) -> Result<()> {
        let mut adding = true;
        let mut applied = String::new();
        let mut unknown = Vec::new();

        for ch in mode_str.chars() {
            match ch {
                '+' => adding = true,
                '-' => adding = false,
                'i' | 'w' | 'o' | 's' => {
                    let mut server_state = ctx.server_state.blocking_write();
                    if let Some(client_state) = server_state.clients.get_mut(&ctx.client_id) {
                        if adding {
                            if client_state.modes.insert(ch) {
                                applied.push('+');
                                applied.push(ch);
                            }
                        } else {
                            if client_state.modes.remove(&ch) {
                                applied.push('-');
                                applied.push(ch);
                            }
                        }
                    }
                }
                _ => {
                    unknown.push(ch);
                }
            }
        }

        if !unknown.is_empty() {
            ctx.send_numeric(501, &[], &format!("Unknown MODE flag(s): {}", unknown.iter().collect::<String>()))?;
        }

        if !applied.is_empty() {
            // Confirm mode change
            let nick = ctx.client_state.nickname.as_deref().unwrap_or("*");
            ctx.send_raw(format!(":{} MODE {} {}", nick, nick, applied))?;
        }

        Ok(())
    }

    /// Execute channel MODE command
    fn execute_channel_mode(&self, ctx: &mut ExecutionContext) -> Result<()> {
        match &self.modes {
            None => {
                // Query channel modes
                // TODO: Implement channel mode query (324 RPL_CHANNELMODEIS)
                ctx.send_numeric(324, &[&self.target, "+nt"], "Channel modes")
            }
            Some(_mode_str) => {
                // Set channel modes
                // TODO: Implement channel mode changes
                // Requires channel ops, channel state, etc.
                ctx.send_numeric(482, &[&self.target], "You're not channel operator")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode_query() {
        let result = ModeCommand::parse(&["MODE", "alice"]);
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd.name(), "MODE");
    }

    #[test]
    fn test_parse_mode_set() {
        let result = ModeCommand::parse(&["MODE", "alice", "+i"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_mode_channel() {
        let result = ModeCommand::parse(&["MODE", "#test", "+m"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_mode_no_target() {
        let result = ModeCommand::parse(&["MODE"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("461"));
    }

    #[test]
    fn test_parse_mode_with_params() {
        let result = ModeCommand::parse(&["MODE", "#test", "+o", "alice"]);
        assert!(result.is_ok());
    }
}
