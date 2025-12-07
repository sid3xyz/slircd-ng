//! NAMES command handler.

use super::super::{HandlerResult, PostRegHandler, server_reply};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

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
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let (nick, _user) = ctx.nick_user();

        // Check if the user has multi-prefix CAP enabled
        let multi_prefix = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
            let user = user.read().await;
            user.caps.contains("multi-prefix")
        } else {
            false
        };

        // NAMES [channel [target]]
        let channel_name = msg.arg(0).unwrap_or("");

        if channel_name.is_empty() {
            // NAMES without channel - list all visible channels
            // Per RFC 2812, list:
            // - Public channels the user is in
            // - Public channels (if user is not in them but they're visible)
            // Secret channels (+s) are not shown unless user is in them
            for channel_arc in ctx.matrix.channels.iter() {
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

                let mut names_list = Vec::new();
                for (uid, member_modes) in members {
                    if let Some(user) = ctx.matrix.users.get(&uid) {
                        let user = user.read().await;
                        let prefix = get_member_prefix(&member_modes, multi_prefix);
                        names_list.push(format!("{}{}", prefix, user.nick));
                    }
                }

                let channel_symbol = if channel_info
                    .modes
                    .contains(&crate::state::actor::ChannelMode::Secret)
                {
                    "@"
                } else {
                    "="
                };

                let names_reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::RPL_NAMREPLY,
                    vec![
                        nick.to_string(),
                        channel_symbol.to_string(),
                        channel_info.name.clone(),
                        names_list.join(" "),
                    ],
                );
                ctx.sender.send(names_reply).await?;
            }

            let reply = server_reply(
                &ctx.matrix.server_info.name,
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

        let channel_lower = irc_to_lower(channel_name);

        if let Some(channel_sender) = ctx.matrix.channels.get(&channel_lower) {
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
                    &ctx.matrix.server_info.name,
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

            let mut names_list = Vec::new();

            for (uid, member_modes) in members {
                if let Some(user) = ctx.matrix.users.get(&uid) {
                    let user = user.read().await;
                    let prefix = get_member_prefix(&member_modes, multi_prefix);
                    names_list.push(format!("{}{}", prefix, user.nick));
                }
            }

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
                &ctx.matrix.server_info.name,
                Response::RPL_NAMREPLY,
                vec![
                    nick.to_string(),
                    channel_symbol.to_string(),
                    channel_info.name.clone(),
                    names_list.join(" "),
                ],
            );
            ctx.sender.send(names_reply).await?;

            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel_info.name.clone(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        } else {
            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel_name.to_string(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        }

        Ok(())
    }
}
