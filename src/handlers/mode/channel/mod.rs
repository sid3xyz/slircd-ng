//! Channel mode handling.
//!
//! Handles MODE commands for channels: `MODE <channel> [+/-modes [args...]]`
//! Supports both simple flags and parameterized modes including list modes.

mod lists;
mod mlock;

use crate::handlers::{
    Context, HandlerError, HandlerResult, resolve_nick_or_nosuchnick, server_reply, with_label,
};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelError, ChannelInfo};
use slirc_proto::{ChannelMode, Mode, Response, irc_to_lower};

use lists::{get_list_mode_query, send_list_mode};
use mlock::apply_mlock_filter;

/// Validation result for a mode requiring an argument.
enum ModeValidation {
    /// Mode is valid, keep it
    Valid,
    /// Mode is invalid, skip it (error already sent)
    Invalid,
    /// Mode has no argument, skip silently
    NoArg,
}

/// Validate a status mode (op/voice/halfop/admin/founder).
///
/// Checks that the target nick exists and is in the channel.
async fn validate_status_mode(
    ctx: &mut Context<'_, RegisteredState>,
    mode: &Mode<ChannelMode>,
    info: &ChannelInfo,
    nick: &str,
    canonical_name: &str,
) -> Result<ModeValidation, HandlerError> {
    let Some(target_nick) = mode.arg() else {
        return Ok(ModeValidation::NoArg);
    };

    let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "MODE", target_nick).await? else {
        return Ok(ModeValidation::Invalid);
    };

    if !info.members.contains(target_uid.as_str()) {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_USERNOTINCHANNEL,
            vec![
                nick.to_string(),
                target_nick.to_string(),
                canonical_name.to_string(),
                "They aren't on that channel".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(ModeValidation::Invalid);
    }

    Ok(ModeValidation::Valid)
}

/// Check if a channel key is valid.
fn is_valid_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(' ') && key.len() <= 23
}

/// Validate a channel key mode.
async fn validate_key_mode(
    ctx: &mut Context<'_, RegisteredState>,
    mode: &Mode<ChannelMode>,
    nick: &str,
    canonical_name: &str,
) -> Result<ModeValidation, HandlerError> {
    if !mode.is_plus() {
        // Removing key is always valid
        return Ok(ModeValidation::Valid);
    }

    let Some(key) = mode.arg() else {
        return Ok(ModeValidation::NoArg);
    };

    // Validate: no spaces, not empty, max 23 chars
    if !is_valid_key(key) {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_INVALIDKEY,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                key.to_string(),
                "Invalid channel key".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(ModeValidation::Invalid);
    }

    Ok(ModeValidation::Valid)
}

/// Validate a channel forward/redirect mode.
///
/// Checks that:
/// - The target channel name is valid (starts with channel prefix)
/// - The target channel exists
/// - The user has channel ops in the target channel (if being set)
async fn validate_channel_target_mode(
    ctx: &mut Context<'_, RegisteredState>,
    mode: &Mode<ChannelMode>,
    nick: &str,
    canonical_name: &str,
    mode_char: char,
) -> Result<ModeValidation, HandlerError> {
    if !mode.is_plus() {
        // Removing target is always valid
        return Ok(ModeValidation::Valid);
    }

    let Some(target) = mode.arg() else {
        return Ok(ModeValidation::NoArg);
    };

    // Validate: target must be a valid channel name (starts with # or &)
    let first_char = target.chars().next();
    if !matches!(first_char, Some('#') | Some('&')) || target.is_empty() || target.len() > 50 {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_INVALIDMODEPARAM,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                mode_char.to_string(),
                target.to_string(),
                "Invalid target channel".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(ModeValidation::Invalid);
    }

    let target_lower = irc_to_lower(target);

    // Check if target channel exists
    if !ctx
        .matrix
        .channel_manager
        .channels
        .contains_key(&target_lower)
    {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_INVALIDMODEPARAM,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                mode_char.to_string(),
                target.to_string(),
                "Target channel does not exist".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(ModeValidation::Invalid);
    }

    // Check if user has channel ops in the target channel
    if let Some(channel_sender) = ctx.matrix.channel_manager.channels.get(&target_lower) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if (channel_sender
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

        if !is_op {
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_CHANOPRIVSNEEDED,
                vec![
                    nick.to_string(),
                    target.to_string(),
                    "You must have channel operator privileges in the target channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(ModeValidation::Invalid);
        }
    }

    Ok(ModeValidation::Valid)
}

/// Validate a channel flood mode.
async fn validate_flood_mode(
    ctx: &mut Context<'_, RegisteredState>,
    mode: &Mode<ChannelMode>,
    nick: &str,
    canonical_name: &str,
) -> Result<ModeValidation, HandlerError> {
    if !mode.is_plus() {
        return Ok(ModeValidation::Valid);
    }

    let Some(param) = mode.arg() else {
        return Ok(ModeValidation::NoArg);
    };

    // Check format: lines:seconds
    let parts: Vec<&str> = param.split(':').collect();
    if parts.len() != 2 || parts[0].parse::<u32>().is_err() || parts[1].parse::<u32>().is_err() {
        let reply = server_reply(
            ctx.server_name(),
            Response::ERR_INVALIDMODEPARAM,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                "f".to_string(),
                param.to_string(),
                "Invalid flood parameter (format: lines:seconds)".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(ModeValidation::Invalid);
    }

    Ok(ModeValidation::Valid)
}

/// Handle channel mode query/change.
pub async fn handle_channel_mode(
    ctx: &mut Context<'_, RegisteredState>,
    channel_name: &str,
    modes: &[Mode<ChannelMode>],
) -> HandlerResult {
    let nick = ctx.nick().to_string();
    let _user_name = ctx.user();
    let channel_lower = irc_to_lower(channel_name);

    // Get channel
    let channel = match ctx.matrix.channel_manager.channels.get(&channel_lower) {
        Some(c) => c.value().clone(),
        None => {
            let reply =
                Response::err_nosuchchannel(&nick, channel_name).with_prefix(ctx.server_prefix());
            ctx.send_error("MODE", "ERR_NOSUCHCHANNEL", reply).await?;
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
            server_reply(ctx.server_name(), Response::RPL_CHANNELMODEIS, params),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        // Also send creation time
        let time_reply = server_reply(
            ctx.server_name(),
            Response::RPL_CREATIONTIME,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                info.created.to_string(),
            ],
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
                .send(ChannelError::ChanOpPrivsNeeded.to_irc_reply(
                    ctx.server_name(),
                    &nick,
                    &canonical_name,
                ))
                .await?;
            return Ok(());
        }

        // Pre-validate modes that require argument validation before applying
        // Filter out invalid modes and send appropriate error messages
        let mut valid_modes = Vec::with_capacity(modes.len());
        for mode in modes {
            let validation = match mode.mode() {
                // Status modes (prefix modes) - validate target exists and is in channel
                ChannelMode::Oper
                | ChannelMode::Voice
                | ChannelMode::Halfop
                | ChannelMode::Admin
                | ChannelMode::Founder => {
                    validate_status_mode(ctx, mode, &info, &nick, &canonical_name).await?
                }
                // Channel key validation
                ChannelMode::Key => validate_key_mode(ctx, mode, &nick, &canonical_name).await?,
                // Channel forwarding/redirect validation
                ChannelMode::JoinForward => {
                    validate_channel_target_mode(ctx, mode, &nick, &canonical_name, 'F').await?
                }
                ChannelMode::Flood => {
                    validate_flood_mode(ctx, mode, &nick, &canonical_name).await?
                }
                ChannelMode::Redirect => {
                    validate_channel_target_mode(ctx, mode, &nick, &canonical_name, 'L').await?
                }
                // All other modes pass through
                _ => ModeValidation::Valid,
            };

            if matches!(validation, ModeValidation::Valid) {
                valid_modes.push(mode.clone());
            }
        }

        if !valid_modes.is_empty() {
            // MLOCK enforcement: filter out modes that conflict with registered channel's MLOCK
            let mlock_filtered_modes = if ctx
                .matrix
                .channel_manager
                .registered_channels
                .contains(&channel_lower)
            {
                apply_mlock_filter(ctx, &channel_lower, valid_modes).await
            } else {
                valid_modes
            };

            if mlock_filtered_modes.is_empty() {
                // All modes were blocked by MLOCK
                return Ok(());
            }

            // Resolve target UIDs for user modes
            let mut target_uids =
                std::collections::HashMap::with_capacity(mlock_filtered_modes.len());
            for mode in &mlock_filtered_modes {
                match mode.mode() {
                    ChannelMode::Oper | ChannelMode::Voice => {
                        if let Some(nick) = mode.arg() {
                            let nick_lower = irc_to_lower(nick);
                            if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) {
                                target_uids.insert(nick.to_string(), uid);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            let (nick, user, host) = if let Some(user_arc) = user_arc {
                let u = user_arc.read().await;
                (u.nick.clone(), u.user.clone(), u.host.clone())
            } else {
                (
                    ctx.state.nick.clone(),
                    ctx.state.user.clone(),
                    "unknown".to_string(),
                )
            };
            let prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            if (channel
                .send(crate::state::actor::ChannelEvent::ApplyModes {
                    params: crate::state::actor::ModeParams {
                        sender_uid: ctx.uid.to_string(),
                        sender_prefix: prefix,
                        modes: mlock_filtered_modes,
                        target_uids,
                        force: false,
                    },
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
                Ok(Err(e)) => {
                    let reply = match e {
                        ChannelError::ChanOpPrivsNeeded => {
                            Response::err_chanoprivsneeded(ctx.server_name(), &canonical_name)
                        }
                        ChannelError::UserNotInChannel(target) => {
                            Response::err_usernotinchannel(ctx.server_name(), &nick, &target)
                        }
                        _ => server_reply(
                            ctx.server_name(),
                            Response::ERR_UNKNOWNERROR,
                            vec![nick.to_string(), canonical_name.to_string(), e.to_string()],
                        ),
                    };
                    ctx.sender.send(reply).await?;
                }
                Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
            }
        }
    }

    Ok(())
}

/// Format applied modes for logging (e.g., "+o+v nick1 nick2").
/// Public so SAMODE can use it for operator confirmation messages.
pub fn format_modes_for_log(modes: &[Mode<ChannelMode>]) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    let mut args = Vec::with_capacity(modes.len());

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_key() {
        assert!(is_valid_key("password"));
        assert!(is_valid_key("12345"));
        assert!(is_valid_key("a".repeat(23).as_str()));

        assert!(!is_valid_key("")); // Empty
        assert!(!is_valid_key("pass word")); // Space
        assert!(!is_valid_key("a".repeat(24).as_str())); // Too long
    }

    #[test]
    fn test_is_valid_key_boundary() {
        assert!(is_valid_key("a".repeat(23).as_str())); // Max length
        assert!(!is_valid_key("a".repeat(24).as_str())); // Max length + 1
    }
}
