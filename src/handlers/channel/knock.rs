//! KNOCK command handler
//!
//! RFC-style extension - Request invite to an invite-only channel

use super::super::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, ChannelError};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use std::time::Instant;
use tokio::sync::oneshot;

/// Minimum seconds between KNOCK requests to the same channel per user.
const KNOCK_COOLDOWN_SECS: u64 = 30;

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

        let server_name = ctx.server_name().to_string();
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
                ctx.send_error("KNOCK", "ERR_NOSUCHCHANNEL", reply).await?;
                return Ok(());
            }
        };

        // Rate limit: prevent KNOCK spam to the same channel
        let now = Instant::now();
        if let Some(last_knock) = ctx.state.knock_timestamps.get(&channel_lower) {
            let elapsed = now.duration_since(*last_knock).as_secs();
            if elapsed < KNOCK_COOLDOWN_SECS {
                let remaining = KNOCK_COOLDOWN_SECS - elapsed;
                let reply = server_reply(
                    &server_name,
                    Response::ERR_TOOMANYKNOCK,
                    vec![
                        nick.clone(),
                        channel_name.to_string(),
                        format!("You must wait {} seconds before knocking again", remaining),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }
        // Record this knock attempt
        ctx.state.knock_timestamps.insert(channel_lower.clone(), now);

        // Prune old entries to prevent unbounded growth (keep last 10 channels)
        if ctx.state.knock_timestamps.len() > 10 {
            let mut entries: Vec<_> = ctx.state.knock_timestamps.iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            entries.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
            entries.truncate(10);
            ctx.state.knock_timestamps = entries.into_iter().collect();
        }

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
                    &server_name,
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
                        &server_name,
                        Response::ERR_CHANOPRIVSNEEDED,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Knocking is disabled on this channel (+K)".to_string(),
                        ],
                    ),
                    ChannelError::ChanOpen => server_reply(
                        &server_name,
                        Response::ERR_CHANOPEN,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "Channel is open, just join it".to_string(),
                        ],
                    ),
                    ChannelError::UserOnChannel(_) => server_reply(
                        &server_name,
                        Response::ERR_KNOCKONCHAN,
                        vec![
                            nick.clone(),
                            channel_name.to_string(),
                            "You're already on that channel".to_string(),
                        ],
                    ),
                    _ => server_reply(
                        &server_name,
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
