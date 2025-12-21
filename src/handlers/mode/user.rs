//! User mode handling.
//!
//! Handles MODE commands for users: `MODE <nick> [+/-modes]`
//! Users can only query/change their own modes.

use super::super::{Context, HandlerResult, server_reply, user_prefix};
use crate::state::{RegisteredState, UserModes};
use slirc_proto::{Command, Message, Mode, Response, UserMode, irc_eq};
use tracing::debug;

/// Handle user mode query/change.
pub async fn handle_user_mode(
    ctx: &mut Context<'_, RegisteredState>,
    target: &str,
    modes: &[Mode<UserMode>],
) -> HandlerResult {
    let nick = ctx.nick();

    // Can only query/change your own modes
    if !irc_eq(target, nick) {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_USERSDONTMATCH,
            vec![
                nick.to_string(),
                "Can't change mode for other users".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(());
    }

    // Get current user modes
    let user_arc = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.value().clone());
    let Some(user) = user_arc else {
        return Ok(());
    };

    if modes.is_empty() {
        // Query: return current modes
        let user = user.read().await;
        let mode_string = user.modes.as_mode_string();
        let reply = server_reply(
            ctx.server_name(),
            Response::RPL_UMODEIS,
            vec![nick.to_string(), mode_string],
        );
        ctx.sender.send(reply).await?;
    } else {
        // Change modes
        let mut user = user.write().await;
        let (applied, rejected) = apply_user_modes_typed(&mut user.modes, modes);

        if !applied.is_empty() {
            // Get host from the user we already have (avoid deadlock)
            let host = user.visible_host.clone();

            // Echo the change back using typed Command::UserMODE
            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(
                    nick,
                    ctx.user(),
                    &host,
                )),
                command: Command::UserMODE(nick.to_string(), applied.clone()),
            };
            ctx.sender.send(mode_msg.clone()).await?;
            debug!(nick = %nick, modes = ?applied, "User modes changed");

            // Notify observer of user update (Innovation 2)
            // We need to drop the lock before notifying to avoid potential deadlocks
            // (though notify_observer reads, so it would be a read-after-write which is fine,
            // but dropping write lock early is good practice).
        }
        drop(user);

        if !applied.is_empty() {
            ctx.matrix.user_manager.notify_observer(ctx.uid, None).await;
        }

        // Report any rejected modes (like +o which only server can set)
        for mode in rejected {
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_UMODEUNKNOWNFLAG,
                vec![nick.to_string(), format!("Unknown mode flag: {}", mode)],
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
    let mut applied = Vec::with_capacity(modes.len());
    let mut rejected = Vec::with_capacity(4);

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
            UserMode::Bot => {
                // +B - mark user as a bot
                user_modes.bot = adding;
                applied.push(mode.clone());
            }
            UserMode::Unknown('T') => {
                // +T - block CTCP messages (except ACTION)
                user_modes.no_ctcp = adding;
                applied.push(mode.clone());
            }
            UserMode::ServerNotices => {
                if let Some(arg) = mode.arg() {
                    for c in arg.chars() {
                        if adding {
                            user_modes.snomasks.insert(c);
                        } else {
                            user_modes.snomasks.remove(&c);
                        }
                    }
                    applied.push(mode.clone());
                } else if !adding {
                    user_modes.snomasks.clear();
                    applied.push(mode.clone());
                } else {
                    // Defaults: c (connect), k (kill), o (oper)
                    user_modes.snomasks.insert('c');
                    user_modes.snomasks.insert('k');
                    user_modes.snomasks.insert('o');
                    applied.push(mode.clone());
                }
            }
            _ => {
                // Unknown/unsupported modes
                rejected.push(mode_type.clone());
            }
        }
    }

    (applied, rejected)
}
