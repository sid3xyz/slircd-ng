//! Channel mode handling.
//!
//! Handles MODE commands for channels: `MODE <channel> [+/-modes [args...]]`
//! Supports both simple flags and parameterized modes including list modes.

mod lists;
mod mlock;

use crate::handlers::{Context, HandlerError, HandlerResult, server_reply, with_label};
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

    let target_lower = irc_to_lower(target_nick);
    let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) else {
        let reply = Response::err_nosuchnick(nick, target_nick).with_prefix(ctx.server_prefix());
        ctx.send_error("MODE", "ERR_NOSUCHNICK", reply).await?;
        return Ok(ModeValidation::Invalid);
    };

    if !info.members.contains(target_uid.value()) {
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
    if key.is_empty() || key.contains(' ') || key.len() > 23 {
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
    let channel = match ctx.matrix.channels.get(&channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = Response::err_nosuchchannel(&nick, channel_name)
                .with_prefix(ctx.server_prefix());
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
            server_reply(
                ctx.server_name(),
                Response::RPL_CHANNELMODEIS,
                params,
            ),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        // Also send creation time
        let time_reply = server_reply(
            ctx.server_name(),
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
        let mut valid_modes = Vec::new();
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
                ChannelMode::Key => {
                    validate_key_mode(ctx, mode, &nick, &canonical_name).await?
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
            let mlock_filtered_modes = if ctx.matrix.registered_channels.contains(&channel_lower) {
                apply_mlock_filter(ctx, &channel_lower, valid_modes).await
            } else {
                valid_modes
            };

            if mlock_filtered_modes.is_empty() {
                // All modes were blocked by MLOCK
                return Ok(());
            }

            // Resolve target UIDs for user modes
            let mut target_uids = std::collections::HashMap::new();
            for mode in &mlock_filtered_modes {
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

            let user_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone());
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
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix: prefix,
                    modes: mlock_filtered_modes,
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
                Ok(Err(e)) => {
                    let reply = match e {
                        ChannelError::ChanOpPrivsNeeded => {
                            Response::err_chanoprivsneeded(ctx.server_name(), &canonical_name)
                        }
                        ChannelError::KeySet => {
                            Response::err_keyset(ctx.server_name(), &canonical_name)
                        }
                        ChannelError::UnknownMode(c, _) => {
                            Response::err_unknownmode(ctx.server_name(), c, &canonical_name)
                        }
                        ChannelError::NoChanModes => {
                            Response::err_nochanmodes(ctx.server_name(), &canonical_name)
                        }
                        ChannelError::BanListFull(c) => {
                            Response::err_banlistfull(ctx.server_name(), &canonical_name, c)
                        }
                        ChannelError::UniqOpPrivsNeeded => {
                            Response::err_uniqopprivsneeded(ctx.server_name())
                        }
                        ChannelError::UserNotInChannel(target) => {
                            Response::err_usernotinchannel(ctx.server_name(), &nick, &target)
                        }
                        _ => server_reply(
                            ctx.server_name(),
                            Response::ERR_UNKNOWNERROR,
                            vec![
                                nick.to_string(),
                                canonical_name.to_string(),
                                e.to_string(),
                            ],
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
