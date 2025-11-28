//! MODE command handler.
//!
//! Handles both user modes and channel modes using slirc-proto's typed MODE parsing.
//!
//! User modes: MODE <nick> [+/-<modes>]
//! Channel modes: MODE <channel> [+/-<modes> [args...]]

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use crate::state::{ListEntry, UserModes};
use async_trait::async_trait;
use slirc_proto::{irc_eq, irc_to_lower, ChannelMode, Command, Message, Mode, Prefix, Response, UserMode};
use tracing::{debug, info};

/// Helper to create a user prefix.
fn user_prefix(nick: &str, user: &str, host: &str) -> Prefix {
    Prefix::Nickname(nick.to_string(), user.to_string(), host.to_string())
}

/// Handler for MODE command.
pub struct ModeHandler;

#[async_trait]
impl Handler for ModeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // slirc-proto parses MODE into typed variants
        match &msg.command {
            Command::UserMODE(target, modes) => {
                handle_user_mode(ctx, target, modes).await
            }
            Command::ChannelMODE(target, modes) => {
                handle_channel_mode(ctx, target, modes).await
            }
            _ => Ok(()),
        }
    }
}

/// Handle user mode query/change.
async fn handle_user_mode(
    ctx: &mut Context<'_>,
    target: &str,
    modes: &[Mode<UserMode>],
) -> HandlerResult {
    let nick = ctx.handshake.nick.as_ref().unwrap();

    // Can only query/change your own modes
    if !irc_eq(target, nick) {
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::ERR_USERSDONTMATCH,
            vec![nick.clone(), "Can't change mode for other users".to_string()],
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
            // Echo the change back
            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(nick, ctx.handshake.user.as_ref().unwrap(), "localhost")),
                command: Command::Raw("MODE".to_string(), vec![nick.clone(), applied.clone()]),
            };
            ctx.sender.send(mode_msg).await?;
            debug!(nick = %nick, modes = %applied, "User modes changed");
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

/// Apply user mode changes from typed modes, returns (applied_string, rejected_modes).
fn apply_user_modes_typed(user_modes: &mut UserModes, modes: &[Mode<UserMode>]) -> (String, Vec<UserMode>) {
    let mut applied = String::new();
    let mut rejected = Vec::new();
    let mut current_dir = ' ';

    for mode in modes {
        let adding = mode.is_plus();
        let mode_type = mode.mode();

        match mode_type {
            UserMode::Invisible => {
                user_modes.invisible = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'i');
            }
            UserMode::Wallops => {
                user_modes.wallops = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'w');
            }
            UserMode::Oper | UserMode::LocalOper => {
                // Oper modes can only be removed, not added by user
                if !adding {
                    user_modes.oper = false;
                    let c = if *mode_type == UserMode::Oper { 'o' } else { 'O' };
                    push_mode(&mut applied, &mut current_dir, adding, c);
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
    let nick = ctx.handshake.nick.as_ref().unwrap();
    let user_name = ctx.handshake.user.as_ref().unwrap();
    let channel_lower = irc_to_lower(channel_name);

    // Get channel
    let channel = match ctx.matrix.channels.get(&channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![nick.clone(), channel_name.to_string(), "No such channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }
    };

    let channel_guard = channel.read().await;
    let canonical_name = channel_guard.name.clone();
    let is_op = channel_guard.is_op(ctx.uid);

    if modes.is_empty() {
        // Query: return current modes
        let mode_string = channel_guard.modes.as_mode_string();
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_CHANNELMODEIS,
            vec![nick.clone(), canonical_name.clone(), mode_string],
        );
        ctx.sender.send(reply).await?;

        // Also send creation time
        let time_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_CREATIONTIME,
            vec![nick.clone(), canonical_name, channel_guard.created.to_string()],
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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_CHANOPRIVSNEEDED,
                vec![nick.clone(), canonical_name, "You're not channel operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let mut channel_guard = channel.write().await;
        let (applied, used_args) = apply_channel_modes_typed(
            ctx,
            &mut channel_guard,
            modes,
        ).await?;

        if !applied.is_empty() {
            // Broadcast the mode change to channel
            let mut mode_params = vec![canonical_name.clone(), applied.clone()];
            mode_params.extend(used_args);

            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(nick, user_name, "localhost")),
                command: Command::Raw("MODE".to_string(), mode_params),
            };

            // Broadcast to all channel members
            for uid in channel_guard.members.keys() {
                if let Some(sender) = ctx.matrix.senders.get(uid) {
                    let _ = sender.send(mode_msg.clone()).await;
                }
            }

            info!(nick = %nick, channel = %canonical_name, modes = %applied, "Channel modes changed");
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
        if matches!(mode_type, 
            ChannelMode::Ban | 
            ChannelMode::Exception | 
            ChannelMode::InviteException | 
            ChannelMode::Quiet
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
    let nick = ctx.handshake.nick.as_ref().unwrap();

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
            vec![nick.clone(), canonical_name.to_string(), end_msg.to_string()],
        );
        ctx.sender.send(end_reply).await?;
    }

    Ok(())
}

/// Apply channel mode changes from typed modes.
/// Returns (applied_string, used_args).
async fn apply_channel_modes_typed(
    ctx: &Context<'_>,
    channel: &mut crate::state::Channel,
    modes: &[Mode<ChannelMode>],
) -> Result<(String, Vec<String>), HandlerError> {
    let mut applied = String::new();
    let mut used_args = Vec::new();
    let mut current_dir = ' ';

    for mode in modes {
        let adding = mode.is_plus();
        let mode_type = mode.mode();
        let arg = mode.arg();

        match mode_type {
            // Simple flags (no parameters)
            ChannelMode::NoExternalMessages => {
                channel.modes.no_external = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'n');
            }
            ChannelMode::ProtectedTopic => {
                channel.modes.topic_lock = adding;
                push_mode(&mut applied, &mut current_dir, adding, 't');
            }
            ChannelMode::InviteOnly => {
                channel.modes.invite_only = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'i');
            }
            ChannelMode::Moderated => {
                channel.modes.moderated = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'm');
            }
            ChannelMode::Secret => {
                channel.modes.secret = adding;
                push_mode(&mut applied, &mut current_dir, adding, 's');
            }
            ChannelMode::RegisteredOnly => {
                channel.modes.registered_only = adding;
                push_mode(&mut applied, &mut current_dir, adding, 'r');
            }
            // Key (+k) - requires parameter to set
            ChannelMode::Key => {
                if adding {
                    if let Some(key) = arg {
                        channel.modes.key = Some(key.to_string());
                        push_mode(&mut applied, &mut current_dir, adding, 'k');
                        used_args.push(key.to_string());
                    }
                } else {
                    channel.modes.key = None;
                    push_mode(&mut applied, &mut current_dir, adding, 'k');
                }
            }
            // Limit (+l) - requires parameter to set
            ChannelMode::Limit => {
                if adding {
                    if let Some(limit_str) = arg
                        && let Ok(limit) = limit_str.parse::<u32>()
                    {
                        channel.modes.limit = Some(limit);
                        push_mode(&mut applied, &mut current_dir, adding, 'l');
                        used_args.push(limit_str.to_string());
                    }
                } else {
                    channel.modes.limit = None;
                    push_mode(&mut applied, &mut current_dir, adding, 'l');
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
                        // Don't add duplicate bans
                        if !channel.bans.iter().any(|b| b.mask == entry.mask) {
                            channel.bans.push(entry);
                            push_mode(&mut applied, &mut current_dir, adding, 'b');
                            used_args.push(mask.to_string());
                        }
                    } else {
                        // Remove ban
                        let before_len = channel.bans.len();
                        channel.bans.retain(|b| b.mask != *mask);
                        if channel.bans.len() != before_len {
                            push_mode(&mut applied, &mut current_dir, adding, 'b');
                            used_args.push(mask.to_string());
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
                            push_mode(&mut applied, &mut current_dir, adding, 'e');
                            used_args.push(mask.to_string());
                        }
                    } else {
                        let before_len = channel.excepts.len();
                        channel.excepts.retain(|b| b.mask != *mask);
                        if channel.excepts.len() != before_len {
                            push_mode(&mut applied, &mut current_dir, adding, 'e');
                            used_args.push(mask.to_string());
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
                            push_mode(&mut applied, &mut current_dir, adding, 'I');
                            used_args.push(mask.to_string());
                        }
                    } else {
                        let before_len = channel.invex.len();
                        channel.invex.retain(|b| b.mask != *mask);
                        if channel.invex.len() != before_len {
                            push_mode(&mut applied, &mut current_dir, adding, 'I');
                            used_args.push(mask.to_string());
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
                            push_mode(&mut applied, &mut current_dir, adding, 'q');
                            used_args.push(mask.to_string());
                        }
                    } else {
                        let before_len = channel.quiets.len();
                        channel.quiets.retain(|b| b.mask != *mask);
                        if channel.quiets.len() != before_len {
                            push_mode(&mut applied, &mut current_dir, adding, 'q');
                            used_args.push(mask.to_string());
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
                            push_mode(&mut applied, &mut current_dir, adding, 'o');
                            used_args.push(target_nick.to_string());
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
                            push_mode(&mut applied, &mut current_dir, adding, 'v');
                            used_args.push(target_nick.to_string());
                        }
                    }
                }
            }
            _ => {
                // Unknown/unsupported mode - ignore
            }
        }
    }

    Ok((applied, used_args))
}

/// Helper to push a mode change to the applied string.
fn push_mode(applied: &mut String, current_dir: &mut char, adding: bool, mode: char) {
    let dir = if adding { '+' } else { '-' };
    if *current_dir != dir {
        applied.push(dir);
        *current_dir = dir;
    }
    applied.push(mode);
}
