//! Channel mode handling.
//!
//! Handles MODE commands for channels: `MODE <channel> [+/-modes [args...]]`
//! Supports both simple flags and parameterized modes including list modes.
//!
//! ## Module Structure
//!
//! This file is intentionally kept as a single unit (~670 lines) because:
//! - All channel mode logic is tightly coupled and shares the same types
//! - The `apply_channel_modes_typed` function handles all modes in one match
//! - Splitting would fragment the mode dispatch logic without clarity benefit
//! - Each mode type section is clearly separated with comments
//!
//! If this file grows beyond 800 lines, consider extracting list mode handling
//! (`send_list_mode`, `get_list_mode_query`) to a separate `channel_lists.rs`.

use super::super::{
    HandlerError, HandlerResult, err_chanoprivsneeded, server_reply, with_label,
};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
use slirc_proto::{ChannelMode, Mode, Response, irc_to_lower};

/// Handle channel mode query/change.
pub async fn handle_channel_mode(
    ctx: &mut TypedContext<'_, Registered>,
    channel_name: &str,
    modes: &[Mode<ChannelMode>],
) -> HandlerResult {
    let nick = ctx.nick();
    let _user_name = ctx.user();
    let channel_lower = irc_to_lower(channel_name);

    // Get channel
    let channel = match ctx.matrix.channels.get(&channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    nick.to_string(),
                    channel_name.to_string(),
                    "No such channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }
    };

    // Get info
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if (channel
        .send(crate::state::actor::ChannelEvent::GetInfo {
            requester_uid: Some(ctx.uid.to_string()),
            reply_tx,
        })
        .await)
        .is_err()
    {
        return Err(HandlerError::Internal("Channel actor died".to_string()));
    }
    let info = match reply_rx.await {
        Ok(i) => i,
        Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
    };

    let canonical_name = info.name.clone();

    // Get member modes
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if (channel
        .send(crate::state::actor::ChannelEvent::GetMemberModes {
            uid: ctx.uid.to_string(),
            reply_tx,
        })
        .await)
        .is_err()
    {
        return Err(HandlerError::Internal("Channel actor died".to_string()));
    }
    let member_modes = match reply_rx.await {
        Ok(m) => m,
        Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
    };
    let is_op = member_modes
        .as_ref()
        .map(|m| m.op || m.admin || m.owner)
        .unwrap_or(false);

    if modes.is_empty() {
        // Query: return current modes
        let mode_string = crate::state::actor::modes_to_string(&info.modes);
        let mut params = vec![nick.to_string(), canonical_name.to_string()];
        if let Some((flags, rest)) = mode_string.split_once(' ') {
            params.push(flags.to_string());
            params.extend(rest.split(' ').map(|s| s.to_string()));
        } else {
            params.push(mode_string);
        }

        let reply = with_label(
            server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_CHANNELMODEIS,
                params,
            ),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        // Also send creation time
        let time_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_CREATIONTIME,
            vec![nick.to_string(), canonical_name.to_string(), info.created.to_string()],
        );
        ctx.sender.send(time_reply).await?;
    } else {
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

        // Pre-validate modes that require argument validation before applying
        // Filter out invalid modes and send appropriate error messages
        let mut valid_modes = Vec::new();
        for mode in modes {
            match mode.mode() {
                // Status modes (prefix modes) - validate target exists and is in channel
                ChannelMode::Oper
                | ChannelMode::Voice
                | ChannelMode::Halfop
                | ChannelMode::Admin
                | ChannelMode::Founder => {
                    if let Some(target_nick) = mode.arg() {
                        let target_lower = irc_to_lower(target_nick);
                        match ctx.matrix.nicks.get(&target_lower) {
                            Some(target_uid) => {
                                // Check if target is in the channel
                                if info.members.contains(target_uid.value()) {
                                    valid_modes.push(mode.clone());
                                } else {
                                    // ERR_USERNOTINCHANNEL (441)
                                    let reply = server_reply(
                                        &ctx.matrix.server_info.name,
                                        Response::ERR_USERNOTINCHANNEL,
                                        vec![
                                            nick.to_string(),
                                            target_nick.to_string(),
                                            canonical_name.clone(),
                                            "They aren't on that channel".to_string(),
                                        ],
                                    );
                                    ctx.sender.send(reply).await?;
                                }
                            }
                            None => {
                                // ERR_NOSUCHNICK (401)
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::ERR_NOSUCHNICK,
                                    vec![
                                        nick.to_string(),
                                        target_nick.to_string(),
                                        "No such nick/channel".to_string(),
                                    ],
                                );
                                ctx.sender.send(reply).await?;
                            }
                        }
                    } else {
                        // Status mode without argument - invalid, skip silently
                    }
                }
                // Channel key validation
                ChannelMode::Key => {
                    if mode.is_plus() {
                        if let Some(key) = mode.arg() {
                            // Validate: no spaces, not empty, max 23 chars
                            if key.is_empty() || key.contains(' ') || key.len() > 23 {
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::ERR_INVALIDKEY,
                                    vec![
                                        nick.to_string(),
                                        canonical_name.clone(),
                                        key.to_string(),
                                        "Invalid channel key".to_string(),
                                    ],
                                );
                                ctx.sender.send(reply).await?;
                            } else {
                                valid_modes.push(mode.clone());
                            }
                        }
                    } else {
                        // Removing key - always valid
                        valid_modes.push(mode.clone());
                    }
                }
                // All other modes pass through
                _ => {
                    valid_modes.push(mode.clone());
                }
            }
        }

        if !valid_modes.is_empty() {
            // Resolve target UIDs for user modes
            let mut target_uids = std::collections::HashMap::new();
            for mode in &valid_modes {
                match mode.mode() {
                    ChannelMode::Oper | ChannelMode::Voice => {
                        if let Some(nick) = mode.arg() {
                            let nick_lower = irc_to_lower(nick);
                            if let Some(uid) = ctx.matrix.nicks.get(&nick_lower) {
                                target_uids.insert(nick.to_string(), uid.value().clone());
                            }
                        }
                    }
                    _ => {}
                }
            }

            let (nick, user, host) = if let Some(u) = ctx.matrix.users.get(ctx.uid) {
                let u = u.read().await;
                (u.nick.clone(), u.user.clone(), u.host.clone())
            } else {
                (
                    ctx.handshake.nick.clone().unwrap_or_default(),
                    ctx.handshake.user.clone().unwrap_or_default(),
                    "unknown".to_string(),
                )
            };
            let prefix = slirc_proto::Prefix::Nickname(nick, user, host);

            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            if (channel
                .send(crate::state::actor::ChannelEvent::ApplyModes {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix: prefix,
                    modes: valid_modes,
                    target_uids,
                    force: false,
                    reply_tx,
                })
                .await)
                .is_err()
            {
                return Err(HandlerError::Internal("Channel actor died".to_string()));
            }

            match reply_rx.await {
                Ok(Ok(_)) => {
                    // Success
                }
                Ok(Err(_e)) => {
                    // Generic error
                    // TODO: Handle specific errors
                }
                Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
            }
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
    ctx: &mut TypedContext<'_, Registered>,
    channel_lower: &str,
    canonical_name: &str,
    list_mode: ChannelMode,
) -> HandlerResult {
    let nick = ctx.nick();

    if let Some(channel) = ctx.matrix.channels.get(channel_lower) {
        // Get the appropriate list and response codes based on mode type
        let (mode_char, reply_code, end_code, end_msg) = match list_mode {
            ChannelMode::Ban => (
                'b',
                Response::RPL_BANLIST,
                Response::RPL_ENDOFBANLIST,
                "End of channel ban list",
            ),
            ChannelMode::Exception => (
                'e',
                Response::RPL_EXCEPTLIST,
                Response::RPL_ENDOFEXCEPTLIST,
                "End of channel exception list",
            ),
            ChannelMode::InviteException => (
                'I',
                Response::RPL_INVITELIST,
                Response::RPL_ENDOFINVITELIST,
                "End of channel invite exception list",
            ),
            ChannelMode::Quiet => (
                'q',
                Response::RPL_QUIETLIST,
                Response::RPL_ENDOFQUIETLIST,
                "End of channel quiet list",
            ),
            _ => return Ok(()),
        };

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if (channel
            .send(crate::state::actor::ChannelEvent::GetList {
                mode: mode_char,
                reply_tx,
            })
            .await)
            .is_err()
        {
            return Err(HandlerError::Internal("Channel actor died".to_string()));
        }

        let list = match reply_rx.await {
            Ok(l) => l,
            Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
        };

        for entry in list {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                reply_code,
                vec![
                    nick.to_string(),
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
                nick.to_string(),
                canonical_name.to_string(),
                end_msg.to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;
    }

    Ok(())
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
