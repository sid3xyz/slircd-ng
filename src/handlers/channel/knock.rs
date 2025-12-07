//! KNOCK command handler
//!
//! RFC-style extension - Request invite to an invite-only channel

use super::super::{HandlerResult, PostRegHandler, server_reply};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tokio::sync::oneshot;

/// Handler for KNOCK command.
///
/// `KNOCK channel [message]`
///
/// Requests an invite to a +i channel.
pub struct KnockHandler;

#[async_trait]
impl PostRegHandler for KnockHandler {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

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
        let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
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
            }
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let sender_prefix = slirc_proto::Prefix::Nickname(nick.clone(), user, host);

        let event = ChannelEvent::Knock {
            sender_uid: ctx.uid.to_string(),
            sender_prefix,
            reply_tx,
        };

        if (channel_tx.send(event).await).is_err() {
            return Ok(());
        }

        match reply_rx.await {
            Ok(Ok(())) => {
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
            }
            Ok(Err(err_code)) => {
                let reply = match err_code.as_str() {
                    "ERR_CANNOTKNOCK" => server_reply(
                        server_name,
                        Response::ERR_CHANOPRIVSNEEDED, // Fallback if ERR_CANNOTKNOCK not available, or use it if available.
                        // Existing code used ERR_CHANOPRIVSNEEDED for +K.
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Knocking is disabled on this channel (+K)".to_string(),
                        ],
                    ),
                    "ERR_CHANOPEN" => server_reply(
                        server_name,
                        Response::ERR_CHANOPEN,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Channel is open, just join it".to_string(),
                        ],
                    ),
                    "ERR_USERONCHANNEL" => server_reply(
                        server_name,
                        Response::ERR_KNOCKONCHAN,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "You're already on that channel".to_string(),
                        ],
                    ),
                    _ => server_reply(
                        server_name,
                        Response::ERR_UNKNOWNERROR,
                        vec![nick.clone(), "Unknown error during KNOCK".to_string()],
                    ),
                };
                ctx.sender.send(reply).await?;
            }
            Err(_) => {}
        }

        Ok(())
    }
}
