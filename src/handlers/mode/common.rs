//! Common mode parsing and validation logic.

use super::super::{Context, HandlerError, server_reply};
use crate::state::RegisteredState;
use slirc_proto::{ChannelMode, Mode, Response, UserMode};
use tracing::debug;

/// Check if target is a channel (starts with #, &, +, or !)
pub fn is_channel_target(target: &str) -> bool {
    matches!(target.chars().next(), Some('#' | '&' | '+' | '!'))
}

/// Parse channel modes from arguments, sending errors to client on failure.
pub async fn parse_channel_modes(
    ctx: &mut Context<'_, RegisteredState>,
    mode_args: &[&str],
) -> Result<Vec<Mode<ChannelMode>>, HandlerError> {
    if mode_args.is_empty() {
        return Ok(vec![]);
    }

    match Mode::as_channel_modes(mode_args) {
        Ok(m) => Ok(m),
        Err(e) => {
            debug!(error = ?e, "Failed to parse channel modes");
            let nick = &ctx.state.nick;
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_UNKNOWNMODE,
                vec![
                    nick.clone(),
                    mode_args.first().copied().unwrap_or("").to_string(),
                    "is unknown mode char to me".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            Ok(vec![])
        }
    }
}

/// Parse user modes from arguments, sending errors to client on failure.
pub async fn parse_user_modes(
    ctx: &mut Context<'_, RegisteredState>,
    mode_args: &[&str],
) -> Result<Vec<Mode<UserMode>>, HandlerError> {
    if mode_args.is_empty() {
        return Ok(vec![]);
    }

    match Mode::as_user_modes(mode_args) {
        Ok(m) => Ok(m),
        Err(e) => {
            debug!(error = ?e, "Failed to parse user modes");
            let nick = &ctx.state.nick;
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_UMODEUNKNOWNFLAG,
                vec![nick.clone(), "Unknown MODE flag".to_string()],
            );
            ctx.sender.send(reply).await?;
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_channel_target() {
        assert!(is_channel_target("#channel"));
        assert!(is_channel_target("&channel"));
        assert!(is_channel_target("+channel"));
        assert!(is_channel_target("!channel"));
        assert!(!is_channel_target("user"));
        assert!(!is_channel_target(""));
    }
}
