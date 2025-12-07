//! User status and profile handlers: AWAY, SETNAME, SILENCE
//!
//! Handles user status management and IRCv3 profile updates.

use super::user_mask_from_state;
use super::{HandlerError, HandlerResult, PostRegHandler, server_reply};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
use async_trait::async_trait;
use slirc_proto::{Command, MessageRef, Response};
use tracing::debug;

/// Handler for AWAY command.
///
/// `AWAY [message]`
///
/// Sets or clears away status and broadcasts to channels for clients with
/// `away-notify`.
pub struct AwayHandler;

#[async_trait]
impl PostRegHandler for AwayHandler {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
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

            // Broadcast AWAY to channels - only to clients with away-notify capability (IRCv3)
            let away_broadcast = slirc_proto::Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::new(&nick, &user_name, &host)),
                command: Command::AWAY(Some(away_text.to_string())),
            };

            for channel_name in &channels {
                ctx.matrix
                    .broadcast_to_channel_with_cap(
                        channel_name,
                        away_broadcast.clone(),
                        None,
                        Some("away-notify"),
                        None, // No fallback - clients without away-notify get nothing
                    )
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

        // Broadcast AWAY (no message) to channels - only to clients with away-notify capability (IRCv3)
        let away_broadcast = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(&nick, &user_name, &host)),
            command: Command::AWAY(None),
        };

        for channel_name in &channels {
            ctx.matrix
                .broadcast_to_channel_with_cap(
                    channel_name,
                    away_broadcast.clone(),
                    None,
                    Some("away-notify"),
                    None, // No fallback - clients without away-notify get nothing
                )
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
impl PostRegHandler for SetnameHandler {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // Check if client has negotiated setname capability
        if !ctx.state.capabilities.contains("setname") {
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
        let (nick, user, visible_host) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.realname = new_realname.to_string();
            (
                user.nick.clone(),
                user.user.clone(),
                user.visible_host.clone(),
            )
        } else {
            return Ok(());
        };

        // Also update handshake state
        ctx.state.realname = Some(new_realname.to_string());

        // Broadcast SETNAME to all channels the user is in (for clients with setname cap)
        let setname_msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(&nick, &user, &visible_host)),
            command: Command::SETNAME(new_realname.to_string()),
        };

        // Get user's channels
        let channels: Vec<String> = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.channels.iter().cloned().collect()
        } else {
            Vec::new()
        };

        // Broadcast to each channel (only to clients with setname capability)
        for channel_name in &channels {
            ctx.matrix
                .broadcast_to_channel_with_cap(
                    channel_name,
                    setname_msg.clone(),
                    None,
                    Some("setname"),
                    None,
                )
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

/// Handler for SILENCE command.
///
/// `SILENCE [+/-mask]`
///
/// Server-side ignore list. Allows users to block messages from matching masks.
/// - Without parameters: Lists the current silence list
/// - With +mask: Adds a mask to the silence list
/// - With -mask: Removes a mask from the silence list
///
/// Masks use standard IRC wildcard syntax: * and ?
pub struct SilenceHandler;

#[async_trait]
impl PostRegHandler for SilenceHandler {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .state
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // SILENCE [+/-mask]
        let mask_arg = msg.arg(0);

        if mask_arg.is_none() {
            // List silence entries
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;

                // RPL_SILELIST (271) for each entry
                for mask in &user.silence_list {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_SILELIST,
                        vec![nick.clone(), mask.clone()],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_ENDOFSILELIST (272)
                let end_reply = server_reply(
                    server_name,
                    Response::RPL_ENDOFSILELIST,
                    vec![nick.clone(), "End of Silence List".to_string()],
                );
                ctx.sender.send(end_reply).await?;
            }
            return Ok(());
        }

        let mask_str = mask_arg.unwrap();

        // Check for +/- prefix
        if mask_str.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let (adding, mask) = if let Some(stripped) = mask_str.strip_prefix('+') {
            (true, stripped)
        } else if let Some(stripped) = mask_str.strip_prefix('-') {
            (false, stripped)
        } else {
            // No prefix, treat as add
            (true, mask_str)
        };

        if mask.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Update silence list
        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;

            if adding {
                // Add to silence list (limit to reasonable size)
                const MAX_SILENCE_ENTRIES: usize = 50;
                if user.silence_list.len() >= MAX_SILENCE_ENTRIES {
                    // ERR_SILELISTFULL (511)
                    let reply = server_reply(
                        server_name,
                        Response::ERR_SILELISTFULL,
                        vec![
                            nick.clone(),
                            mask.to_string(),
                            format!("Your silence list is full (max {})", MAX_SILENCE_ENTRIES),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }

                if user.silence_list.insert(mask.to_string()) {
                    debug!(nick = %nick, mask = %mask, "Added to silence list");
                }
            } else {
                // Remove from silence list
                if user.silence_list.remove(mask) {
                    debug!(nick = %nick, mask = %mask, "Removed from silence list");
                }
            }
        }

        Ok(())
    }
}
