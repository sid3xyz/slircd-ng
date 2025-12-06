//! INVITE command handler
//!
//! RFC 2812 - Channel invitation

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_notonchannel,
    err_notregistered, server_reply, user_mask_from_state,
};
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response, irc_to_lower};
use tokio::sync::oneshot;

/// Handler for INVITE command.
///
/// `INVITE nickname channel`
///
/// Invites a user to a channel.
pub struct InviteHandler;

#[async_trait]
impl Handler for InviteHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // INVITE <nickname> <channel> or INVITE <channel> <nickname>
        // Detect which argument is which based on whether it starts with a channel prefix
        let arg0 = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let arg1 = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        // Track if channel was first (non-standard order) for echo
        let channel_first = arg0.starts_with('#')
            || arg0.starts_with('&')
            || arg0.starts_with('+')
            || arg0.starts_with('!');

        let (target_nick, channel_name) = if channel_first {
            // INVITE #channel nickname format
            (arg1, arg0)
        } else {
            // INVITE nickname #channel format (standard)
            (arg0, arg1)
        };

        let channel_lower = irc_to_lower(channel_name);
        let target_lower = irc_to_lower(target_nick);

        // Check if target exists
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        "No such nick/channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if channel exists
        if let Some(channel_tx) = ctx.matrix.channels.get(&channel_lower) {
            // Check if user is on channel
            let user_in_channel = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
                let user = user.read().await;
                user.channels.contains(&channel_lower)
            } else {
                false
            };

            if !user_in_channel {
                ctx.sender
                    .send(err_notonchannel(server_name, nick, channel_name))
                    .await?;
                return Ok(());
            }

            let (reply_tx, reply_rx) = oneshot::channel();
            let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;
            let sender_prefix = slirc_proto::Prefix::Nickname(nick.clone(), user, host);

            let event = ChannelEvent::Invite {
                sender_uid: ctx.uid.to_string(),
                sender_prefix: sender_prefix.clone(),
                target_uid: target_uid.clone(),
                target_nick: target_nick.to_string(),
                force: false,
                reply_tx,
            };

            if (channel_tx.send(event).await).is_err() {
                return Ok(());
            }

            match reply_rx.await {
                Ok(Ok(())) => {
                    // Success, invite recorded in channel.
                    // Now send INVITE message to target user.

                    let invite_msg = Message {
                        tags: None,
                        prefix: Some(sender_prefix),
                        command: if channel_first {
                            Command::INVITE(channel_name.to_string(), target_nick.to_string())
                        } else {
                            Command::INVITE(target_nick.to_string(), channel_name.to_string())
                        },
                    };

                    if let Some(target_sender) = ctx.matrix.senders.get(&target_uid) {
                        let _ = target_sender.send(invite_msg).await;
                    }

                    // RPL_INVITING (341)
                    let reply = server_reply(
                        server_name,
                        Response::RPL_INVITING,
                        vec![
                            nick.clone(),
                            target_nick.to_string(),
                            channel_name.to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }
                Ok(Err(err_code)) => {
                    let reply = match err_code.as_str() {
                        "ERR_CHANOPRIVSNEEDED" => {
                            err_chanoprivsneeded(server_name, &nick, channel_name)
                        }
                        "ERR_USERONCHANNEL" => server_reply(
                            server_name,
                            Response::ERR_USERONCHANNEL,
                            vec![
                                nick.clone(),
                                target_nick.to_string(),
                                channel_name.to_string(),
                                "is already on channel".to_string(),
                            ],
                        ),
                        _ => server_reply(
                            server_name,
                            Response::ERR_UNKNOWNERROR,
                            vec![nick.clone(), "Unknown error during INVITE".to_string()],
                        ),
                    };
                    ctx.sender.send(reply).await?;
                }
                Err(_) => {}
            }
        } else {
            // Channel doesn't exist - RFC1459/2812 allows invites to non-existent channels
            // "There is no requirement that the channel the target user is being
            // invited to must exist or be a valid channel."

            let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;
            let sender_prefix = slirc_proto::Prefix::Nickname(nick.clone(), user, host);

            // Send INVITE notification to target
            let invite_msg = Message {
                tags: None,
                prefix: Some(sender_prefix.clone()),
                command: if channel_first {
                    Command::INVITE(channel_name.to_string(), target_nick.to_string())
                } else {
                    Command::INVITE(target_nick.to_string(), channel_name.to_string())
                },
            };

            if let Some(target_sender) = ctx.matrix.senders.get(&target_uid) {
                let _ = target_sender.send(invite_msg).await;
            }

            // Echo INVITE back to sender
            let echo_msg = Message {
                tags: None,
                prefix: Some(sender_prefix),
                command: if channel_first {
                    Command::INVITE(channel_name.to_string(), target_nick.to_string())
                } else {
                    Command::INVITE(target_nick.to_string(), channel_name.to_string())
                },
            };
            ctx.sender.send(echo_msg).await?;
        }

        Ok(())
    }
}
