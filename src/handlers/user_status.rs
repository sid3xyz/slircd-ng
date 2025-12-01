//! User status and profile handlers: AWAY, SETNAME
//!
//! Handles user status management and IRCv3 profile updates.

use super::{Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply};
use async_trait::async_trait;
use slirc_proto::{Command, MessageRef, Response};
use tracing::debug;

/// Handler for AWAY command.
///
/// `AWAY [message]`
///
/// Sets or clears away status per RFC 2812.
/// - With a message: Sets the user as away with that reason.
/// - Without a message (or empty): Clears the away status.
pub struct AwayHandler;

#[async_trait]
impl Handler for AwayHandler {
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

        // AWAY [message]
        let away_msg = msg.arg(0);

        if let Some(away_text) = away_msg
            && !away_text.is_empty()
        {
            // Get list of channels before setting away status (for away-notify)
            let channels = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                user.channels.iter().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            // Set away status
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let mut user = user_ref.write().await;
                user.away = Some(away_text.to_string());
            }

            // Broadcast AWAY to channels with away-notify capability (IRCv3)
            let away_broadcast = slirc_proto::Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::new(
                    nick,
                    ctx.handshake
                        .user
                        .as_ref()
                        .ok_or(HandlerError::NickOrUserMissing)?,
                    "localhost",
                )),
                command: Command::AWAY(Some(away_text.to_string())),
            };

            for channel_name in &channels {
                ctx.matrix
                    .broadcast_to_channel(channel_name, away_broadcast.clone(), None)
                    .await;
            }

            // RPL_NOWAWAY (306)
            debug!(nick = %nick, away = %away_text, "User marked as away");
            let reply = server_reply(
                server_name,
                Response::RPL_NOWAWAY,
                vec![
                    nick.clone(),
                    "You have been marked as being away".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Get list of channels before clearing away status (for away-notify)
        let channels = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.channels.iter().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Clear away status
        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.away = None;
        }

        // Broadcast AWAY (no message) to channels with away-notify capability (IRCv3)
        let away_broadcast = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(
                nick,
                ctx.handshake
                    .user
                    .as_ref()
                    .ok_or(HandlerError::NickOrUserMissing)?,
                "localhost",
            )),
            command: Command::AWAY(None),
        };

        for channel_name in &channels {
            ctx.matrix
                .broadcast_to_channel(channel_name, away_broadcast.clone(), None)
                .await;
        }

        // RPL_UNAWAY (305)
        debug!(nick = %nick, "User no longer away");
        let reply = server_reply(
            server_name,
            Response::RPL_UNAWAY,
            vec![
                nick.clone(),
                "You are no longer marked as being away".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for SETNAME command (IRCv3).
///
/// `SETNAME <new realname>`
///
/// Allows users to change their realname (gecos) after connection.
/// Requires the `setname` capability to be negotiated.
/// Reference: <https://ircv3.net/specs/extensions/setname>
pub struct SetnameHandler;

#[async_trait]
impl Handler for SetnameHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        // Check if client has negotiated setname capability
        if !ctx.handshake.capabilities.contains("setname") {
            debug!("SETNAME rejected: client has not negotiated setname capability");
            return Ok(());
        }

        let new_realname = match msg.arg(0) {
            Some(name) if !name.is_empty() => name,
            _ => {
                // FAIL SETNAME INVALID_REALNAME :Realname is not valid
                let fail = slirc_proto::Message {
                    tags: None,
                    prefix: None,
                    command: Command::Raw(
                        "FAIL".to_string(),
                        vec![
                            "SETNAME".to_string(),
                            "INVALID_REALNAME".to_string(),
                            "Realname is not valid".to_string(),
                        ],
                    ),
                };
                ctx.sender.send(fail).await?;
                return Ok(());
            }
        };

        // Update the user's realname
        let (nick, user, host) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.realname = new_realname.to_string();
            (user.nick.clone(), user.user.clone(), user.host.clone())
        } else {
            return Ok(());
        };

        // Also update handshake state
        ctx.handshake.realname = Some(new_realname.to_string());

        // Broadcast SETNAME to all channels the user is in (for clients with setname cap)
        let setname_msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(&nick, &user, &host)),
            command: Command::SETNAME(new_realname.to_string()),
        };

        // Get user's channels
        let channels: Vec<String> = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.channels.iter().cloned().collect()
        } else {
            Vec::new()
        };

        // Broadcast to each channel (including back to sender for echo)
        for channel_name in &channels {
            ctx.matrix
                .broadcast_to_channel(channel_name, setname_msg.clone(), None)
                .await;
        }

        // Also echo back to the sender if not in any channels
        if channels.is_empty() {
            ctx.sender.send(setname_msg).await?;
        }

        debug!(nick = %nick, new_realname = %new_realname, "User changed realname via SETNAME");

        Ok(())
    }
}
