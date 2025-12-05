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
    Context, HandlerError, HandlerResult, err_chanoprivsneeded, server_reply, user_mask_from_state,
    user_prefix, with_label,
};
use crate::security::ExtendedBan;
use crate::state::ListEntry;
use slirc_proto::{ChannelMode, Command, Message, Mode, Response, irc_to_lower};
use tracing::info;

/// Handle channel mode query/change.
pub async fn handle_channel_mode(
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
        // Send modes and parameters as separate IRC params (no combined trailing)
        let mode_string = channel_guard.modes.as_mode_string();
        let mut params = vec![nick.clone(), canonical_name.clone()];
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
                                // Nick exists - check if they're in the channel
                                if channel_guard.is_member(target_uid.value()) {
                                    valid_modes.push(mode.clone());
                                } else {
                                    // ERR_USERNOTINCHANNEL (441)
                                    let reply = server_reply(
                                        &ctx.matrix.server_info.name,
                                        Response::ERR_USERNOTINCHANNEL,
                                        vec![
                                            nick.clone(),
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
                                        nick.clone(),
                                        target_nick.to_string(),
                                        "No such nick/channel".to_string(),
                                    ],
                                );
                                ctx.sender.send(reply).await?;
                            }
                        }
                    } else {
                        // Status mode without argument - invalid, skip silently
                        // (parser should have caught this, but be defensive)
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
                                        nick.clone(),
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

        let applied_modes = apply_channel_modes_typed(ctx, &mut channel_guard, &valid_modes)?;

        if !applied_modes.is_empty() {
            // Broadcast the mode change to channel using typed Command
            let (_, _, host) = user_mask_from_state(ctx, ctx.uid)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;

            let mode_msg = Message {
                tags: None,
                prefix: Some(user_prefix(nick, user_name, &host)),
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
                        // Check if issuer can modify target
                        // For self-modification: only allow removing privileges, not granting
                        let can_modify = if ctx.uid == target_uid {
                            !adding // Can only de-op yourself
                        } else {
                            channel.can_modify(ctx.uid, &target_uid)
                        };

                        if can_modify
                            && let Some(member_modes) = channel.members.get_mut(&target_uid)
                        {
                            member_modes.op = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Oper, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Oper, Some(target_nick.to_string()))
                            });
                        }
                        // Silently ignore if hierarchy check fails
                    }
                }
            }
            // Voice (+v)
            ChannelMode::Voice => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        // Check if issuer can modify target
                        // For voice: if you have halfop or higher, you can voice yourself or others
                        // This is a lower privilege so self-grant is allowed for privileged users
                        let issuer_modes = channel.members.get(ctx.uid).cloned();
                        let can_modify = if ctx.uid == target_uid {
                            // Self-modification: allow if removing, or if user has halfop+ (can grant voice)
                            !adding || issuer_modes.is_some_and(|m| m.has_halfop_or_higher())
                        } else {
                            channel.can_modify(ctx.uid, &target_uid)
                        };

                        if can_modify
                            && let Some(member_modes) = channel.members.get_mut(&target_uid)
                        {
                            member_modes.voice = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Voice, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Voice, Some(target_nick.to_string()))
                            });
                        }
                        // Silently ignore if hierarchy check fails
                    }
                }
            }
            // Halfop (+h)
            ChannelMode::Halfop => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        // Check if issuer can modify target
                        // For self-modification: only allow removing privileges, not granting
                        let can_modify = if ctx.uid == target_uid {
                            !adding // Can only de-halfop yourself
                        } else {
                            channel.can_modify(ctx.uid, &target_uid)
                        };

                        if can_modify
                            && let Some(member_modes) = channel.members.get_mut(&target_uid)
                        {
                            member_modes.halfop = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Halfop, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Halfop, Some(target_nick.to_string()))
                            });
                        }
                        // Silently ignore if hierarchy check fails
                    }
                }
            }
            // Admin (+a)
            ChannelMode::Admin => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        // Check if issuer can modify target
                        // For self-modification: only allow removing privileges, not granting
                        let can_modify = if ctx.uid == target_uid {
                            !adding // Can only de-admin yourself
                        } else {
                            channel.can_modify(ctx.uid, &target_uid)
                        };

                        if can_modify
                            && let Some(member_modes) = channel.members.get_mut(&target_uid)
                        {
                            member_modes.admin = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Admin, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Admin, Some(target_nick.to_string()))
                            });
                        }
                        // Silently ignore if hierarchy check fails
                    }
                }
            }
            // Owner/Founder (+q/+Q)
            ChannelMode::Founder => {
                if let Some(target_nick) = arg {
                    let target_lower = irc_to_lower(target_nick);
                    if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                        let target_uid = target_uid.value().clone();
                        // Check if issuer can modify target
                        // For self-modification: only allow removing privileges, not granting
                        let can_modify = if ctx.uid == target_uid {
                            !adding // Can only de-owner yourself
                        } else {
                            channel.can_modify(ctx.uid, &target_uid)
                        };

                        if can_modify
                            && let Some(member_modes) = channel.members.get_mut(&target_uid)
                        {
                            member_modes.owner = adding;
                            applied_modes.push(if adding {
                                Mode::Plus(ChannelMode::Founder, Some(target_nick.to_string()))
                            } else {
                                Mode::Minus(ChannelMode::Founder, Some(target_nick.to_string()))
                            });
                        }
                        // Silently ignore if hierarchy check fails
                    }
                }
            }
            // SLIRCd advanced channel protection modes (via Unknown variant)
            ChannelMode::Unknown('f') => {
                // Flood protection: +f lines:seconds
                if adding {
                    if let Some(param) = arg
                        && let Some((lines, secs)) = parse_colon_pair(param)
                    {
                        channel.modes.flood_limit = Some((lines, secs));
                        applied_modes.push(Mode::Plus(
                            ChannelMode::Unknown('f'),
                            Some(param.to_string()),
                        ));
                    }
                } else {
                    channel.modes.flood_limit = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Unknown('f'), None));
                }
            }
            ChannelMode::Unknown('L') => {
                // Channel redirect: +L #channel
                if adding {
                    if let Some(target) = arg {
                        // Validate target is a channel name
                        if target.starts_with('#')
                            || target.starts_with('&')
                            || target.starts_with('+')
                            || target.starts_with('!')
                        {
                            channel.modes.redirect = Some(target.to_string());
                            applied_modes.push(Mode::Plus(
                                ChannelMode::Unknown('L'),
                                Some(target.to_string()),
                            ));
                        }
                    }
                } else {
                    channel.modes.redirect = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Unknown('L'), None));
                }
            }
            ChannelMode::Unknown('j') => {
                // Join throttle: +j count:seconds
                if adding {
                    if let Some(param) = arg
                        && let Some((count, secs)) = parse_colon_pair(param)
                    {
                        channel.modes.join_throttle = Some((count, secs));
                        applied_modes.push(Mode::Plus(
                            ChannelMode::Unknown('j'),
                            Some(param.to_string()),
                        ));
                    }
                } else {
                    channel.modes.join_throttle = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Unknown('j'), None));
                }
            }
            ChannelMode::Unknown('J') => {
                // Join delay: +J seconds
                if adding {
                    if let Some(secs_str) = arg
                        && let Ok(secs) = secs_str.parse::<u32>()
                    {
                        channel.modes.join_delay = Some(secs);
                        applied_modes.push(Mode::Plus(
                            ChannelMode::Unknown('J'),
                            Some(secs_str.to_string()),
                        ));
                    }
                } else {
                    channel.modes.join_delay = None;
                    applied_modes.push(Mode::Minus(ChannelMode::Unknown('J'), None));
                }
            }
            // Extended channel modes (simple flags)
            ChannelMode::NoColors => {
                channel.modes.no_colors = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoColors, None)
                } else {
                    Mode::Minus(ChannelMode::NoColors, None)
                });
            }
            ChannelMode::NoCTCP => {
                channel.modes.no_ctcp = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoCTCP, None)
                } else {
                    Mode::Minus(ChannelMode::NoCTCP, None)
                });
            }
            ChannelMode::NoNickChange => {
                channel.modes.no_nick_change = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoNickChange, None)
                } else {
                    Mode::Minus(ChannelMode::NoNickChange, None)
                });
            }
            ChannelMode::NoKnock => {
                channel.modes.no_knock = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoKnock, None)
                } else {
                    Mode::Minus(ChannelMode::NoKnock, None)
                });
            }
            ChannelMode::NoInvite => {
                channel.modes.no_invite = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoInvite, None)
                } else {
                    Mode::Minus(ChannelMode::NoInvite, None)
                });
            }
            ChannelMode::NoChannelNotice => {
                channel.modes.no_channel_notice = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoChannelNotice, None)
                } else {
                    Mode::Minus(ChannelMode::NoChannelNotice, None)
                });
            }
            ChannelMode::NoKick => {
                channel.modes.no_kick = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::NoKick, None)
                } else {
                    Mode::Minus(ChannelMode::NoKick, None)
                });
            }
            ChannelMode::Permanent => {
                channel.modes.permanent = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::Permanent, None)
                } else {
                    Mode::Minus(ChannelMode::Permanent, None)
                });
            }
            ChannelMode::OperOnly => {
                channel.modes.oper_only = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::OperOnly, None)
                } else {
                    Mode::Minus(ChannelMode::OperOnly, None)
                });
            }
            ChannelMode::FreeInvite => {
                channel.modes.free_invite = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::FreeInvite, None)
                } else {
                    Mode::Minus(ChannelMode::FreeInvite, None)
                });
            }
            // TLS-only channel (+z)
            ChannelMode::TlsOnly => {
                channel.modes.tls_only = adding;
                applied_modes.push(if adding {
                    Mode::Plus(ChannelMode::TlsOnly, None)
                } else {
                    Mode::Minus(ChannelMode::TlsOnly, None)
                });
            }
            _ => {
                // Unknown/unsupported mode - ignore
            }
        }
    }

    Ok(applied_modes)
}

/// Parse a "number:number" format parameter (e.g., "5:10" for flood/throttle modes).
fn parse_colon_pair(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        let first = parts[0].parse::<u32>().ok()?;
        let second = parts[1].parse::<u32>().ok()?;
        Some((first, second))
    } else {
        None
    }
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
