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
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.value().clone());
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
                prefix: Some(user_prefix(nick, ctx.user(), &host)),
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
            UserMode::HideChannels => {
                // +p - Hide channels in WHOIS
                user_modes.hide_channels = adding;
                applied.push(mode.clone());
            }
            UserMode::Deaf => {
                // +d - Deaf mode
                user_modes.deaf = adding;
                applied.push(mode.clone());
            }
            UserMode::CallerId => {
                // +g - CallerID
                user_modes.caller_id = adding;
                applied.push(mode.clone());
            }
            UserMode::NetAdmin => {
                // +N - Network Admin (protected)
                if !adding {
                    user_modes.net_admin = false;
                    applied.push(mode.clone());
                } else {
                    rejected.push(mode_type.clone());
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::{Mode, UserMode};

    #[test]
    fn test_apply_user_modes_simple() {
        let mut modes = UserModes::default();
        let changes = vec![
            Mode::Plus(UserMode::Invisible, None),
            Mode::Plus(UserMode::Wallops, None),
        ];

        let (applied, rejected) = apply_user_modes_typed(&mut modes, &changes);

        assert!(modes.invisible);
        assert!(modes.wallops);
        assert_eq!(applied.len(), 2);
        assert!(rejected.is_empty());
    }

    #[test]
    fn test_apply_user_modes_remove() {
        let mut modes = UserModes::default();
        modes.invisible = true;

        let changes = vec![Mode::Minus(UserMode::Invisible, None)];
        let (applied, _) = apply_user_modes_typed(&mut modes, &changes);

        assert!(!modes.invisible);
        assert_eq!(applied.len(), 1);
    }

    #[test]
    fn test_apply_user_modes_rejected() {
        let mut modes = UserModes::default();
        // Operator mode cannot be set by user via MODE command (usually)
        // But apply_user_modes_typed logic allows it if passed?
        // Let's check the implementation.
        // It seems apply_user_modes_typed handles Operator mode by setting it!
        // Wait, the handler calls this. The handler doesn't filter it?
        // Ah, apply_user_modes_typed implementation:
        /*
            UserMode::Oper => {
                // Cannot set +o via MODE, only via OPER command
                if !adding && user_modes.oper {
                    user_modes.oper = false;
                    applied.push(mode.clone());
                } else {
                    rejected.push(mode_type.clone());
                }
            }
        */
        // So +o should be rejected.

        let changes = vec![Mode::Plus(UserMode::Oper, None)];
        let (applied, rejected) = apply_user_modes_typed(&mut modes, &changes);

        assert!(!modes.oper);
        assert!(applied.is_empty());
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn test_apply_user_modes_snomask() {
        let mut modes = UserModes::default();

        // +s with arg
        let changes = vec![Mode::Plus(UserMode::ServerNotices, Some("ck".to_string()))];
        apply_user_modes_typed(&mut modes, &changes);
        assert!(modes.snomasks.contains(&'c'));
        assert!(modes.snomasks.contains(&'k'));

        // -s with arg
        let changes = vec![Mode::Minus(UserMode::ServerNotices, Some("c".to_string()))];
        apply_user_modes_typed(&mut modes, &changes);
        assert!(!modes.snomasks.contains(&'c'));
        assert!(modes.snomasks.contains(&'k'));
    }

    #[test]
    fn test_apply_user_modes_unknown() {
        let mut modes = UserModes::default();
        let changes = vec![Mode::Plus(UserMode::Unknown('?'), None)];
        let (applied, rejected) = apply_user_modes_typed(&mut modes, &changes);

        assert!(applied.is_empty());
        assert_eq!(rejected.len(), 1);
        assert!(matches!(rejected[0], UserMode::Unknown('?')));
    }

    #[test]
    fn test_apply_user_modes_ctcp() {
        let mut modes = UserModes::default();
        // +T = no_ctcp
        // Wait, UserMode::Unknown('T') is handled specially in apply_user_modes_typed
        /*
            UserMode::Unknown('T') => {
                // +T - block CTCP messages (except ACTION)
                user_modes.no_ctcp = adding;
                applied.push(mode.clone());
            }
        */

        let changes = vec![Mode::Plus(UserMode::Unknown('T'), None)];
        let (applied, _) = apply_user_modes_typed(&mut modes, &changes);
        assert!(modes.no_ctcp);
        assert_eq!(applied.len(), 1);

        let changes = vec![Mode::Minus(UserMode::Unknown('T'), None)];
        apply_user_modes_typed(&mut modes, &changes);
        assert!(!modes.no_ctcp);
    }
}
