//! KNOCK command handler
//!
//! RFC-style extension - Request invite to an invite-only channel

use super::super::{Context, Handler, HandlerResult, err_notregistered, server_reply};
use async_trait::async_trait;
use slirc_proto::{Command, MessageRef, Prefix, Response, irc_to_lower};

/// Handler for KNOCK command.
///
/// `KNOCK channel [message]`
///
/// Requests an invite to a +i channel.
pub struct KnockHandler;

#[async_trait]
impl Handler for KnockHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        // KNOCK <channel> [message]
        let channel_name = match msg.arg(0) {
            Some(c) if !c.is_empty() => c,
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = &ctx.matrix.server_info.name;
                let nick = {
                    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                        let user = user_ref.read().await;
                        user.nick.clone()
                    } else {
                        "*".to_string()
                    }
                };

                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![
                        nick,
                        "KNOCK".to_string(),
                        "Not enough parameters".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };
        let knock_msg = msg.arg(1);

        let server_name = &ctx.matrix.server_info.name;
        let channel_lower = irc_to_lower(channel_name);

        // Get user info
        let (nick, user, host) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let u = user_ref.read().await;
                (u.nick.clone(), u.user.clone(), u.host.clone())
            } else {
                return Ok(());
            }
        };

        // Check if channel exists
        let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    nick,
                    channel_name.to_string(),
                    "No such channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Check if user is already in channel
        {
            let channel = channel_ref.read().await;
            if channel.is_member(ctx.uid) {
                // ERR_KNOCKONCHAN (714) - already on channel
                let reply = server_reply(
                    server_name,
                    Response::ERR_KNOCKONCHAN,
                    vec![
                        nick,
                        channel_name.to_string(),
                        "You're already on that channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
            // Check if channel is +i (invite only)
            if !channel.modes.invite_only {
                // ERR_CHANOPEN (713) - channel not invite-only
                let reply = server_reply(
                    server_name,
                    Response::ERR_CHANOPEN,
                    vec![
                        nick.clone(),
                        channel_name.to_string(),
                        "Channel is open, just join it".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Build KNOCK notification for channel ops
        let knock_text = knock_msg
            .map(|s| s.to_string())
            .unwrap_or_else(|| "has asked for an invite".to_string());
        let knock_notice = slirc_proto::Message {
            tags: None,
            prefix: Some(Prefix::Nickname(nick.clone(), user, host)),
            command: Command::KNOCK(channel_name.to_string(), Some(knock_text)),
        };

        // Send to channel operators
        {
            let channel = channel_ref.read().await;
            for (member_uid, modes) in &channel.members {
                if modes.op
                    && let Some(sender) = ctx.matrix.senders.get(member_uid)
                {
                    let _ = sender.send(knock_notice.clone()).await;
                }
            }
        }

        // RPL_KNOCKDLVR (711) - knock delivered
        let reply = server_reply(
            server_name,
            Response::RPL_KNOCKDLVR,
            vec![
                nick,
                channel_name.to_string(),
                "Your knock has been delivered".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
