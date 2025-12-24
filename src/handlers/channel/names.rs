//! NAMES command handler.
//!
//! This implements RFC 2812 (Modern) format for NAMES replies.
//! RFC 1459 format (without channel symbol) is deprecated and not supported.

use super::super::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

fn parse_names_target<'a>(msg: &MessageRef<'a>) -> Option<&'a str> {
    match msg.arg(0) {
        Some(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Handler for NAMES command.
pub struct NamesHandler;

/// Build prefix string for a member based on whether multi-prefix is enabled.
fn get_member_prefix(member_modes: &crate::state::MemberModes, multi_prefix: bool) -> String {
    if multi_prefix {
        member_modes.all_prefix_chars()
    } else if let Some(prefix) = member_modes.prefix_char() {
        prefix.to_string()
    } else {
        String::new()
    }
}

#[async_trait]
impl PostRegHandler for NamesHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let (nick, _user) = ctx.nick_user();

        // Check if the user has multi-prefix CAP enabled
        let multi_prefix = if let Some(user) = ctx.matrix.user_manager.users.get(ctx.uid) {
            let user = user.read().await;
            user.caps.contains("multi-prefix")
        } else {
            false
        };

        // NAMES [channel [target]]
        let target_channel = parse_names_target(msg);

        if target_channel.is_none() {
            // NAMES without channel - list all visible channels
            // Per RFC 2812, list:
            // - Public channels the user is in
            // - Public channels (if user is not in them but they're visible)
            // Secret channels (+s) are not shown unless user is in them

            // Collect and sort channels alphabetically for deterministic output
            let mut channel_names: Vec<_> = ctx
                .matrix
                .channel_manager
                .channels
                .iter()
                .map(|entry| entry.key().clone())
                .collect();
            channel_names.sort();

            // Result limiting to prevent flooding
            let max_channels = ctx.matrix.config.limits.max_names_channels;
            let mut result_count = 0;
            let mut truncated = false;

            for channel_lower in channel_names {
                // Check result limit
                if result_count >= max_channels {
                    truncated = true;
                    break;
                }
                let Some(channel_arc) = ctx.matrix.channel_manager.channels.get(&channel_lower)
                else {
                    continue;
                };
                let sender = channel_arc.value();
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = sender
                    .send(crate::state::actor::ChannelEvent::GetInfo {
                        requester_uid: Some(ctx.uid.to_string()),
                        reply_tx: tx,
                    })
                    .await;

                let channel_info = match rx.await {
                    Ok(info) => info,
                    Err(_) => continue,
                };

                // Skip secret channels unless user is a member
                if channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Secret)
                    && !channel_info.is_member
                {
                    continue;
                }

                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = sender
                    .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
                    .await;
                let members = match rx.await {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let mut names_list = Vec::with_capacity(members.len());

                let is_auditorium = channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Auditorium);
                let requester_privileged = if let Some(modes) = members.get(&ctx.uid.to_string()) {
                    modes.voice || modes.halfop || modes.op || modes.admin || modes.owner
                } else {
                    false
                };

                for (uid, member_modes) in &members {
                    // Auditorium filtering: if +u and requester is not privileged,
                    // they only see privileged users.
                    if is_auditorium && !requester_privileged {
                        let is_target_privileged = member_modes.voice
                            || member_modes.halfop
                            || member_modes.op
                            || member_modes.admin
                            || member_modes.owner;

                        if !is_target_privileged {
                            continue;
                        }
                    }

                    if let Some(user) = ctx.matrix.user_manager.users.get(uid) {
                        let user = user.read().await;
                        let prefix = get_member_prefix(member_modes, multi_prefix);
                        names_list.push((user.nick.clone(), format!("{}{}", prefix, user.nick)));
                    }
                }
                // Sort alphabetically by nick (case-insensitive) for deterministic output
                names_list.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                let names_list: Vec<String> =
                    names_list.into_iter().map(|(_, display)| display).collect();

                let channel_symbol = if channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Secret)
                {
                    "@"
                } else {
                    "="
                };

                let names_reply = server_reply(
                    ctx.server_name(),
                    Response::RPL_NAMREPLY,
                    vec![
                        nick.to_string(),
                        channel_symbol.to_string(),
                        channel_info.name.clone(),
                        names_list.join(" "),
                    ],
                );
                ctx.sender.send(names_reply).await?;
                result_count += 1;
            }

            // Notify if results were truncated
            if truncated {
                let notice = server_reply(
                    ctx.server_name(),
                    Response::RPL_TRYAGAIN,
                    vec![
                        nick.to_string(),
                        "NAMES".to_string(),
                        format!("Output truncated, {} channels max", max_channels),
                    ],
                );
                ctx.sender.send(notice).await?;
            }

            let reply = server_reply(
                ctx.server_name(),
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    "*".to_string(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_name = target_channel.unwrap();
        // Handle multiple channels (comma-separated): NAMES #chan1,#chan2
        let channels: Vec<&str> = channel_name.split(',').collect();

        for (idx, chan) in channels.iter().enumerate() {
            let channel_lower = irc_to_lower(chan);

            if let Some(channel_sender) = ctx.matrix.channel_manager.channels.get(&channel_lower) {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = channel_sender
                    .send(crate::state::actor::ChannelEvent::GetInfo {
                        requester_uid: Some(ctx.uid.to_string()),
                        reply_tx: tx,
                    })
                    .await;

                let channel_info = match rx.await {
                    Ok(info) => info,
                    Err(_) => return Ok(()),
                };

                // If channel is secret and user is not a member, treat as if it doesn't exist
                if channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Secret)
                    && !channel_info.is_member
                {
                    let end_names = server_reply(
                        ctx.server_name(),
                        Response::RPL_ENDOFNAMES,
                        vec![
                            nick.to_string(),
                            channel_name.to_string(),
                            "End of /NAMES list".to_string(),
                        ],
                    );
                    ctx.sender.send(end_names).await?;
                    return Ok(());
                }

                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = channel_sender
                    .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
                    .await;
                let members = match rx.await {
                    Ok(m) => m,
                    Err(_) => return Ok(()),
                };

                let mut names_list = Vec::with_capacity(members.len());

                let is_auditorium = channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Auditorium);
                let requester_privileged = if let Some(modes) = members.get(&ctx.uid.to_string()) {
                    modes.voice || modes.halfop || modes.op || modes.admin || modes.owner
                } else {
                    false
                };

                for (uid, member_modes) in &members {
                    // Auditorium filtering: if +u and requester is not privileged,
                    // they only see privileged users.
                    if is_auditorium && !requester_privileged {
                        let is_target_privileged = member_modes.voice
                            || member_modes.halfop
                            || member_modes.op
                            || member_modes.admin
                            || member_modes.owner;

                        if !is_target_privileged {
                            continue;
                        }
                    }

                    if let Some(user) = ctx.matrix.user_manager.users.get(uid) {
                        let user = user.read().await;
                        let prefix = get_member_prefix(member_modes, multi_prefix);
                        names_list.push((user.nick.clone(), format!("{}{}", prefix, user.nick)));
                    }
                }
                // Sort alphabetically by nick (case-insensitive) for deterministic output
                names_list.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                let names_list: Vec<String> =
                    names_list.into_iter().map(|(_, display)| display).collect();

                // Channel symbol per RFC 2812:
                // @ = secret (+s)
                // * = private (not used, some IRCds treat +p this way)
                // = = public (default)
                let channel_symbol = if channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Secret)
                {
                    "@"
                } else {
                    "="
                };

                let names_reply = server_reply(
                    ctx.server_name(),
                    Response::RPL_NAMREPLY,
                    vec![
                        nick.to_string(),
                        channel_symbol.to_string(),
                        channel_info.name.clone(),
                        names_list.join(" "),
                    ],
                );
                ctx.sender.send(names_reply).await?;

                // Only send RPL_ENDOFNAMES after the last channel
                if idx == channels.len() - 1 {
                    let end_names = server_reply(
                        ctx.server_name(),
                        Response::RPL_ENDOFNAMES,
                        vec![
                            nick.to_string(),
                            channel_name.to_string(), // Original comma-separated list
                            "End of /NAMES list".to_string(),
                        ],
                    );
                    ctx.sender.send(end_names).await?;
                }
            } else {
                // Channel doesn't exist - only send RPL_ENDOFNAMES if it's the last channel
                if idx == channels.len() - 1 {
                    let end_names = server_reply(
                        ctx.server_name(),
                        Response::RPL_ENDOFNAMES,
                        vec![
                            nick.to_string(),
                            channel_name.to_string(), // Original comma-separated list
                            "End of /NAMES list".to_string(),
                        ],
                    );
                    ctx.sender.send(end_names).await?;
                }
            }
        } // end for loop

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::MessageRef;
    use crate::state::MemberModes;

    #[test]
    fn test_parse_names_target_none() {
        let msg = MessageRef::parse("NAMES").unwrap();
        assert_eq!(parse_names_target(&msg), None);
    }

    #[test]
    fn test_parse_names_target_empty() {
        let msg = MessageRef::parse("NAMES :").unwrap();
        assert_eq!(parse_names_target(&msg), None);
    }

    #[test]
    fn test_parse_names_target_specific() {
        let msg = MessageRef::parse("NAMES #channel").unwrap();
        assert_eq!(parse_names_target(&msg), Some("#channel"));
    }

    #[test]
    fn test_get_member_prefix_single() {
        let mut modes = MemberModes::default();
        modes.op = true;
        assert_eq!(get_member_prefix(&modes, false), "@");

        modes.voice = true; // op > voice
        assert_eq!(get_member_prefix(&modes, false), "@");
    }

    #[test]
    fn test_get_member_prefix_multi() {
        let mut modes = MemberModes::default();
        modes.op = true;
        modes.voice = true;
        assert_eq!(get_member_prefix(&modes, true), "@+");
    }

    #[test]
    fn test_get_member_prefix_none() {
        let modes = MemberModes::default();
        assert_eq!(get_member_prefix(&modes, false), "");
    }
}
