//! KNOCK command handler
//!
//! RFC-style extension - Request invite to an invite-only channel

use super::super::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, ChannelError};
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
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // KNOCK <channel> [message]
        let channel_name = match msg.arg(0) {
            Some(c) if !c.is_empty() => c,
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = ctx.server_name();
                let nick = {
                    if let Some(user_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone()) {
                        let user = user_arc.read().await;
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

        let server_name = ctx.server_name();
        let channel_lower = irc_to_lower(channel_name);

        // Get user info
        let (nick, user, host) = {
            if let Some(user_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone()) {
                let u = user_arc.read().await;
                (u.nick.clone(), u.user.clone(), u.host.clone())
            } else {
                return Ok(());
            }
        };

        // Check if channel exists
        let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = Response::err_nosuchchannel(&nick, channel_name)
                    .with_prefix(ctx.server_prefix());
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("KNOCK", "ERR_NOSUCHCHANNEL");
                return Ok(());
            }
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

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
            Ok(Err(e)) => {
                let reply = match e {
                    ChannelError::CannotKnock => server_reply(
                        server_name,
                        Response::ERR_CHANOPRIVSNEEDED,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Knocking is disabled on this channel (+K)".to_string(),
                        ],
                    ),
                    ChannelError::ChanOpen => server_reply(
                        server_name,
                        Response::ERR_CHANOPEN,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Channel is open, just join it".to_string(),
                        ],
                    ),
                    ChannelError::UserOnChannel(_) => server_reply(
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
                        vec![nick.clone(), e.to_string()],
                    ),
                };
                ctx.sender.send(reply).await?;
            }
            Err(_) => {}
        }

        Ok(())
    }
}
