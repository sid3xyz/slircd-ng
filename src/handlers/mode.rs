//! MODE command handler.
//!
//! Handles both user modes and channel modes using slirc-proto's typed MODE parsing.
//!
//! - User modes: `MODE nick [+/-modes]`
//! - Channel modes: `MODE channel [+/-modes [args...]]`

use super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, server_reply, user_prefix,
    with_label,
};
use crate::security::ExtendedBan;
use crate::state::{ListEntry, UserModes};
use async_trait::async_trait;
use slirc_proto::{
    ChannelMode, Command, Message, MessageRef, Mode, Response, UserMode, irc_eq, irc_to_lower,
};
use tracing::{debug, info};

/// Handler for MODE command.
pub struct ModeHandler;

#[async_trait]
impl Handler for ModeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // MODE <target> [modes [params]]
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let server_name = &ctx.matrix.server_info.name;

        // Determine if this is a user or channel mode based on target
        if is_channel_target(target) {
            // Parse channel modes from args
            let mode_args: Vec<&str> = msg.args().iter().skip(1).copied().collect();
            let modes = if mode_args.is_empty() {
                vec![]
            } else {
                match Mode::as_channel_modes(&mode_args) {
                    Ok(m) => m,
                    Err(e) => {
                        debug!(error = ?e, "Failed to parse channel modes");
                        // Send ERR_UNKNOWNMODE for malformed modes
                        let reply = server_reply(
                            server_name,
                            Response::ERR_UNKNOWNMODE,
                            vec![
                                nick.clone(),
                                mode_args.first().copied().unwrap_or("").to_string(),
                                "is unknown mode char to me".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }
            };
            handle_channel_mode(ctx, target, &modes).await
        } else {
            // Parse user modes from args
            let mode_args: Vec<&str> = msg.args().iter().skip(1).copied().collect();
            let modes = if mode_args.is_empty() {
                vec![]
            } else {
                match Mode::as_user_modes(&mode_args) {
                    Ok(m) => m,
                    Err(e) => {
                        debug!(error = ?e, "Failed to parse user modes");
                        // Send ERR_UMODEUNKNOWNFLAG for malformed user modes
                        let reply = server_reply(
                            server_name,
                            Response::ERR_UMODEUNKNOWNFLAG,
                            vec![nick.clone(), "Unknown MODE flag".to_string()],
                        );
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }
            };
            handle_user_mode(ctx, target, &modes).await
        }
    }
}

/// Check if target is a channel (starts with #, &, +, or !)
fn is_channel_target(target: &str) -> bool {
    matches!(target.chars().next(), Some('#' | '&' | '+' | '!'))
}

/// Handle user mode query/change.
async fn handle_user_mode(
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
            ctx.sender.send(mode_msg).await?;
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
fn apply_user_modes_typed(
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
            _ => {
                // Unknown/unsupported modes
                rejected.push(mode_type.clone());
            }
        }
    }

    (applied, rejected)
}

/// Handle channel mode query/change.
async fn handle_channel_mode(
    ctx: &mut Context<'_>,
    channel_name: &str,
    modes: &[Mode<ChannelMode>],
) -> HandlerResult {
    let nick = ctx
        .handshake
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let user_name = ctx
        .handshake
        .user
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let channel_lower = irc_to_lower(channel_name);

    // Get channel
    let channel = match ctx.matrix.channels.get(&channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "No such channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }
    };

    let channel_guard = channel.read().await;
    let canonical_name = channel_guard.name.clone();
    let is_op = channel_guard.is_op(ctx.uid);

    if modes.is_empty() {
        // Query: return current modes - attach label for labeled-response
        let mode_string = channel_guard.modes.as_mode_string();
        let reply = with_label(
            server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_CHANNELMODEIS,
                vec![nick.clone(), canonical_name.clone(), mode_string],
            ),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        // Also send creation time
        let time_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_CREATIONTIME,
            vec![
                nick.clone(),
                canonical_name,
                channel_guard.created.to_string(),
            ],
        );
        ctx.sender.send(time_reply).await?;
    } else {
        drop(channel_guard);

        // Check for list mode queries (Type A modes with no argument)
        // Users can query these lists without being op
        if let Some(list_query) = get_list_mode_query(modes) {
            return send_list_mode(ctx, &channel_lower, &canonical_name, list_query).await;
        }

        // Must be op to change modes
        if !is_op {
            ctx.sender
                .send(err_chanoprivsneeded(
                    &ctx.matrix.server_info.name,
                    nick,
                    &canonical_name,
                ))
                .await?;
            return Ok(());
        }

        let mut channel_guard = channel.write().await;
        let applied_modes = apply_channel_modes_typed(ctx, &mut channel_guard, modes)?;

        if !applied_modes.is_empty() {
            // Broadcast the mode change to channel using typed Command
            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(nick, user_name, "localhost")),
                command: Command::ChannelMODE(canonical_name.clone(), applied_modes.clone()),
            };

            // Broadcast to all channel members
            for uid in channel_guard.members.keys() {
                if let Some(sender) = ctx.matrix.senders.get(uid) {
                    let _ = sender.send(mode_msg.clone()).await;
                }
            }

            info!(nick = %nick, channel = %canonical_name, modes = %format_modes_for_log(&applied_modes), "Channel modes changed");
        }
    }

    Ok(())
}

/// Check if this is a list mode query (Type A mode with no argument).
/// Returns the list mode type if it's a query, None otherwise.
fn get_list_mode_query(modes: &[Mode<ChannelMode>]) -> Option<ChannelMode> {
    if modes.len() == 1 && modes[0].arg().is_none() {
        let mode_type = modes[0].mode();
        // Type A (list) modes: Ban, Exception, InviteException, Quiet
        if matches!(
            mode_type,
            ChannelMode::Ban
                | ChannelMode::Exception
                | ChannelMode::InviteException
                | ChannelMode::Quiet
        ) {
            return Some(mode_type.clone());
        }
    }
    None
}

/// Send a list mode's entries for a channel.
async fn send_list_mode(
    ctx: &mut Context<'_>,
    channel_lower: &str,
    canonical_name: &str,
    list_mode: ChannelMode,
) -> HandlerResult {
    let nick = ctx
        .handshake
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;

    if let Some(channel) = ctx.matrix.channels.get(channel_lower) {
        let channel = channel.read().await;

        // Get the appropriate list and response codes based on mode type
        let (list, reply_code, end_code, end_msg) = match list_mode {
            ChannelMode::Ban => (
                &channel.bans,
                Response::RPL_BANLIST,
                Response::RPL_ENDOFBANLIST,
                "End of channel ban list",
            ),
            ChannelMode::Exception => (
                &channel.excepts,
                Response::RPL_EXCEPTLIST,
                Response::RPL_ENDOFEXCEPTLIST,
                "End of channel exception list",
            ),
            ChannelMode::InviteException => (
                &channel.invex,
                Response::RPL_INVITELIST,
                Response::RPL_ENDOFINVITELIST,
                "End of channel invite exception list",
            ),
            ChannelMode::Quiet => (
                &channel.quiets,
                Response::RPL_QUIETLIST,
                Response::RPL_ENDOFQUIETLIST,
                "End of channel quiet list",
            ),
            _ => return Ok(()), // Should never happen
        };

        for entry in list {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                reply_code,
                vec![
                    nick.clone(),
                    canonical_name.to_string(),
                    entry.mask.clone(),
                    entry.set_by.clone(),
                    entry.set_at.to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        let end_reply = server_reply(
            &ctx.matrix.server_info.name,
            end_code,
            vec![
                nick.clone(),
                canonical_name.to_string(),
                end_msg.to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;
    }

    Ok(())
}

/// Apply channel mode changes from typed modes.
/// Returns the successfully applied modes as typed `Mode<ChannelMode>` for use with
/// `Command::ChannelMODE`, ensuring wire-format correctness via slirc-proto serialization.
///
/// This is public so SAMODE can reuse the mode application logic.
#[allow(clippy::result_large_err)] // HandlerError contains large SendError variant
pub fn apply_channel_modes_typed(
    ctx: &Context<'_>,
    channel: &mut crate::state::Channel,
    modes: &[Mode<ChannelMode>],
) -> Result<Vec<Mode<ChannelMode>>, HandlerError> {
    let mut applied_modes = Vec::new();

    for mode in modes {
        let adding = mode.is_plus();
        let mode_type = mode.mode();
        let arg = mode.arg();

        match mode_type {
            // Simple flags (no parameters)
            ChannelMode::NoExternalMessages => {
                channel.modes.no_external = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoExternalMessages, None)
                } else {
                    Mode::Minus(ChannelMode::NoExternalMessages, None)
                });
            }
            ChannelMode::ProtectedTopic => {
                channel.modes.topic_lock = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::ProtectedTopic, None)
                } else {
                    Mode::Minus(ChannelMode::ProtectedTopic, None)
                });
            }
            ChannelMode::InviteOnly => {
                channel.modes.invite_only = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::InviteOnly, None)
                } else {
                    Mode::Minus(ChannelMode::InviteOnly, None)
                });
            }
            ChannelMode::Moderated => {
                channel.modes.moderated = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::Moderated, None)
                } else {
                    Mode::Minus(ChannelMode::Moderated, None)
                });
            }
            ChannelMode::Secret => {
                channel.modes.secret = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::Secret, None)
                } else {
                    Mode::Minus(ChannelMode::Secret, None)
                });
            }
            ChannelMode::RegisteredOnly => {
                channel.modes.registered_only = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::RegisteredOnly, None)
                } else {
                    Mode::Minus(ChannelMode::RegisteredOnly, None)
                });
            }
            // Key (+k) - requires parameter to set
            ChannelMode::Key => {
                if adding {
                    if let Some(key) = arg {
                        channel.modes.key = Some(key.to_string());
                        applied_modes.push(Mode::Plus(ChannelMode::Key, Some(key.to_string())));
                    }
                } else {
                    channel.modes.key = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Key, None));
                }
            }
            // Limit (+l) - requires parameter to set
            ChannelMode::Limit => {
                if adding {
                    if let Some(limit_str) = arg
                        && let Ok(limit) = limit_str.parse::<u32>()
                    {
                        channel.modes.limit = Some(limit);
                        applied_modes
                            .push(Mode::Plus(ChannelMode::Limit, Some(limit_str.to_string())));
                    }
                } else {
                    channel.modes.limit = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Limit, None));
                }
            }
            // Ban (+b)
            ChannelMode::Ban => {
                if let Some(mask) = arg {
                    if adding {
                        let entry = ListEntry {
                            mask: mask.to_string(),
                            set_by: ctx.handshake.nick.clone().unwrap_or_default(),
                            set_at: chrono::Utc::now().timestamp(),
                        };

                        // Check if this is an extended ban (starts with $)
                        if mask.starts_with('$') {
                            // Try to parse as extended ban
                            if ExtendedBan::parse(mask).is_some() {
                                // Valid extended ban - add to extended_bans list
                                if !channel.extended_bans.iter().any(|b| b.mask == entry.mask) {
                                    channel.extended_bans.push(entry);
                                    applied_modes
                                        .push(Mode::Plus(ChannelMode::Ban, Some(mask.to_string())));
                                }
                            } else {
                                // Invalid extended ban format - treat as regular hostmask
                                if !channel.bans.iter().any(|b| b.mask == entry.mask) {
                                    channel.bans.push(entry);
                                    applied_modes
                                        .push(Mode::Plus(ChannelMode::Ban, Some(mask.to_string())));
                                }
                            }
                        } else {
                            // Regular hostmask ban
                            if !channel.bans.iter().any(|b| b.mask == entry.mask) {
                                channel.bans.push(entry);
                                applied_modes
                                    .push(Mode::Plus(ChannelMode::Ban, Some(mask.to_string())));
                            }
                        }
                    } else {
                        // Remove ban from both lists
                        let before_len = channel.bans.len();
                        channel.bans.retain(|b| b.mask != *mask);
                        let removed_normal = channel.bans.len() != before_len;

                        let before_len_ext = channel.extended_bans.len();
                        channel.extended_bans.retain(|b| b.mask != *mask);
                        let removed_extended = channel.extended_bans.len() != before_len_ext;

                        if removed_normal || removed_extended {
                            applied_modes
                                .push(Mode::Minus(ChannelMode::Ban, Some(mask.to_string())));
                        }
                    }
                }
            }
            // Ban exception (+e)
            ChannelMode::Exception => {
                if let Some(mask) = arg {
                    if adding {
                        let entry = ListEntry {
                            mask: mask.to_string(),
                            set_by: ctx.handshake.nick.clone().unwrap_or_default(),
                            set_at: chrono::Utc::now().timestamp(),
                        };
                        if !channel.excepts.iter().any(|b| b.mask == entry.mask) {
                            channel.excepts.push(entry);
                            applied_modes
                                .push(Mode::Plus(ChannelMode::Exception, Some(mask.to_string())));
                        }
                    } else {
                        let before_len = channel.excepts.len();
                        channel.excepts.retain(|b| b.mask != *mask);
                        if channel.excepts.len() != before_len {
                            applied_modes
                                .push(Mode::Minus(ChannelMode::Exception, Some(mask.to_string())));
                        }
                    }
                }
            }
            // Invite exception (+I)
            ChannelMode::InviteException => {
                if let Some(mask) = arg {
                    if adding {
                        let entry = ListEntry {
                            mask: mask.to_string(),
                            set_by: ctx.handshake.nick.clone().unwrap_or_default(),
                            set_at: chrono::Utc::now().timestamp(),
                        };
                        if !channel.invex.iter().any(|b| b.mask == entry.mask) {
                            channel.invex.push(entry);
                            applied_modes.push(Mode::Plus(
                                ChannelMode::InviteException,
                                Some(mask.to_string()),
                            ));
                        }
                    } else {
                        let before_len = channel.invex.len();
                        channel.invex.retain(|b| b.mask != *mask);
                        if channel.invex.len() != before_len {
                            applied_modes.push(Mode::Minus(
                                ChannelMode::InviteException,
                                Some(mask.to_string()),
                            ));
                        }
                    }
                }
            }
            // Quiet (+q)
            ChannelMode::Quiet => {
                if let Some(mask) = arg {
                    if adding {
                        let entry = ListEntry {
                            mask: mask.to_string(),
                            set_by: ctx.handshake.nick.clone().unwrap_or_default(),
                            set_at: chrono::Utc::now().timestamp(),
                        };
                        if !channel.quiets.iter().any(|b| b.mask == entry.mask) {
                            channel.quiets.push(entry);
                            applied_modes
                                .push(Mode::Plus(ChannelMode::Quiet, Some(mask.to_string())));
                        }
                    } else {
                        let before_len = channel.quiets.len();
                        channel.quiets.retain(|b| b.mask != *mask);
                        if channel.quiets.len() != before_len {
                            applied_modes
                                .push(Mode::Minus(ChannelMode::Quiet, Some(mask.to_string())));
                        }
                    }
                }
            }
            // Op (+o)
            ChannelMode::Oper => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        if let Some(member_modes) = channel.members.get_mut(&target_uid) {
                            member_modes.op = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Oper, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Oper, Some(target_nick.to_string()))
                            });
                        }
                    }
                }
            }
            // Voice (+v)
            ChannelMode::Voice => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        if let Some(member_modes) = channel.members.get_mut(&target_uid) {
                            member_modes.voice = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Voice, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Voice, Some(target_nick.to_string()))
                            });
                        }
                    }
                }
            }
            _ => {
                // Unknown/unsupported mode - ignore
            }
        }
    }

    Ok(applied_modes)
}

/// Format applied modes for logging (e.g., "+o+v nick1 nick2").
/// Public so SAMODE can use it for operator confirmation messages.
pub fn format_modes_for_log(modes: &[Mode<ChannelMode>]) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    let mut args = Vec::new();

    for mode in modes {
        let _ = write!(result, "{}", mode.flag());
        if let Some(arg) = mode.arg() {
            args.push(arg);
        }
    }

    for arg in args {
        result.push(' ');
        result.push_str(arg);
    }

    result
}
