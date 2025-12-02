//! User mode handling.
//!
//! Handles MODE commands for users: `MODE <nick> [+/-modes]`
//! Users can only query/change their own modes.

use super::super::{Context, HandlerError, HandlerResult, server_reply, user_prefix};
use crate::state::UserModes;
use slirc_proto::{Command, Message, Mode, Response, UserMode, irc_eq};
use tracing::debug;

/// Handle user mode query/change.
pub async fn handle_user_mode(
    ctx: &mut Context<'_>,
    target: &str,
    modes: &[Mode<UserMode>],
) -> HandlerResult {
    let nick = ctx
        .handshake
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;

    // Can only query/change your own modes
    if !irc_eq(target, nick) {
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::ERR_USERSDONTMATCH,
            vec![
                nick.clone(),
                "Can't change mode for other users".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(());
    }

    // Get current user modes
    let user = match ctx.matrix.users.get(ctx.uid) {
        Some(u) => u.clone(),
        None => return Ok(()),
    };

    if modes.is_empty() {
        // Query: return current modes
        let user = user.read().await;
        let mode_string = user.modes.as_mode_string();
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_UMODEIS,
            vec![nick.clone(), mode_string],
        );
        ctx.sender.send(reply).await?;
    } else {
        // Change modes
        let mut user = user.write().await;
        let (applied, rejected) = apply_user_modes_typed(&mut user.modes, modes);

        if !applied.is_empty() {
            // Echo the change back using typed Command::UserMODE
            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(
                    nick,
                    ctx.handshake
                        .user
                        .as_ref()
                        .ok_or(HandlerError::NickOrUserMissing)?,
                    "localhost",
                )),
                command: Command::UserMODE(nick.clone(), applied.clone()),
            };
            ctx.sender.send(mode_msg.clone()).await?;
            debug!(nick = %nick, modes = ?applied, "User modes changed");
        }

        // Report any rejected modes (like +o which only server can set)
        for mode in rejected {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_UMODEUNKNOWNFLAG,
                vec![nick.clone(), format!("Unknown mode flag: {}", mode)],
            );
            ctx.sender.send(reply).await?;
        }
    }

    Ok(())
}

/// Apply user mode changes from typed modes, returns (applied_modes, rejected_modes).
pub fn apply_user_modes_typed(
    user_modes: &mut UserModes,
    modes: &[Mode<UserMode>],
) -> (Vec<Mode<UserMode>>, Vec<UserMode>) {
    let mut applied = Vec::new();
    let mut rejected = Vec::new();

    for mode in modes {
        let adding = mode.is_plus();
        let mode_type = mode.mode();

        match mode_type {
            UserMode::Invisible => {
                user_modes.invisible = adding;
                applied.push(mode.clone());
            }
            UserMode::Wallops => {
                user_modes.wallops = adding;
                applied.push(mode.clone());
            }
            UserMode::Registered => {
                // +r can only be set by server (via NickServ)
                if !adding {
                    user_modes.registered = false;
                    applied.push(mode.clone());
                } else {
                    rejected.push(mode_type.clone());
                }
            }
            UserMode::Oper | UserMode::LocalOper => {
                // Oper modes can only be removed, not added by user
                if !adding {
                    user_modes.oper = false;
                    applied.push(mode.clone());
                } else {
                    rejected.push(mode_type.clone());
                }
            }
            UserMode::MaskedHost => {
                // +x is set by server, can't be changed by user
                rejected.push(mode_type.clone());
            }
            UserMode::RegisteredOnly => {
                // +R - only accept PMs from registered users
                user_modes.registered_only = adding;
                applied.push(mode.clone());
            }
            _ => {
                // Unknown/unsupported modes
                rejected.push(mode_type.clone());
            }
        }
    }

    (applied, rejected)
}
