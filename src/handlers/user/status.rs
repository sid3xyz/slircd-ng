//! User status and profile handlers: AWAY, SETNAME, SILENCE
//!
//! Handles user status management and IRCv3 profile updates.

use super::monitor::notify_extended_monitor_watchers;
use crate::handlers::user_mask_from_state;
use crate::handlers::{Context, HandlerError, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
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
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        let away_msg = msg.arg(0).filter(|s| !s.is_empty());

        // Get list of channels before updating status (for away-notify)
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        let channels = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.channels.iter().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Update away status
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            let mut user = user_arc.write().await;
            user.away = away_msg.map(ToString::to_string);
        }

        // Notify observer of user update (Innovation 2)
        ctx.matrix.user_manager.notify_observer(ctx.uid, None).await;

        // Broadcast AWAY to channels - only to clients with away-notify capability (IRCv3)
        let away_broadcast = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(&nick, &user_name, &host)),
            command: Command::AWAY(away_msg.map(ToString::to_string)),
        };

        use crate::handlers::util::helpers::broadcast_user_update;
        broadcast_user_update(
            ctx,
            &away_broadcast,
            &channels,
            Some("away-notify"),
            None, // No exclude - clients might want to see their own away status? (Original code used None)
        )
        .await;

        // Extended MONITOR: Notify watchers with extended-monitor + away-notify
        notify_extended_monitor_watchers(ctx.matrix, &nick, away_broadcast, "away-notify").await;

        // Send reply
        if let Some(text) = away_msg {
            // Setting away
            debug!(nick = %nick, away = %text, "User marked as away");
            let reply = server_reply(
                server_name,
                Response::RPL_NOWAWAY,
                vec![
                    nick.clone(),
                    "You have been marked as being away".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        } else {
            // Clearing away
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
        }

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
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // Check if client has negotiated setname capability
        if !ctx.state.capabilities.contains("setname") {
            debug!("SETNAME rejected: client has not negotiated setname capability");
            return Ok(());
        }

        // Maximum realname length (common server limit, prevents abuse)
        const MAX_REALNAME_LEN: usize = 128;

        let new_realname = match msg.arg(0) {
            Some(name) if !name.is_empty() && name.len() <= MAX_REALNAME_LEN => name,
            Some(name) if name.len() > MAX_REALNAME_LEN => {
                // FAIL SETNAME INVALID_REALNAME :Realname too long (max 128 chars)
                let fail = slirc_proto::Message {
                    tags: None,
                    prefix: None,
                    command: Command::FAIL(
                        "SETNAME".to_string(),
                        "INVALID_REALNAME".to_string(),
                        vec![format!("Realname too long (max {} chars)", MAX_REALNAME_LEN)],
                    ),
                };
                ctx.sender.send(fail).await?;
                return Ok(());
            }
            _ => {
                // FAIL SETNAME INVALID_REALNAME :Realname is not valid
                let fail = slirc_proto::Message {
                    tags: None,
                    prefix: None,
                    command: Command::FAIL(
                        "SETNAME".to_string(),
                        "INVALID_REALNAME".to_string(),
                        vec!["Realname is not valid".to_string()],
                    ),
                };
                ctx.sender.send(fail).await?;
                return Ok(());
            }
        };

        // Update the user's realname
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        let (nick, user, visible_host) = if let Some(user_arc) = user_arc {
            let mut user = user_arc.write().await;
            user.realname = new_realname.to_string();
            (
                user.nick.clone(),
                user.user.clone(),
                user.visible_host.clone(),
            )
        } else {
            return Ok(());
        };

        // Also update session state
        ctx.state.realname = new_realname.to_string();

        // Broadcast SETNAME to all channels the user is in (for clients with setname cap)
        let setname_msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new(&nick, &user, &visible_host)),
            command: Command::SETNAME(new_realname.to_string()),
        };

        // Get user's channels
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        let channels: Vec<String> = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.channels.iter().cloned().collect()
        } else {
            Vec::new()
        };

        // Always echo back to the sender to confirm the change.
        // Do this synchronously (not via channel broadcast) so it is observed before
        // any synchronize PING/PONG used by the test harness.
        ctx.sender.send(setname_msg.clone()).await?;

        // Broadcast to each channel (only to clients with setname capability)
        use crate::handlers::util::helpers::broadcast_user_update;
        broadcast_user_update(
            ctx,
            &setname_msg,
            &channels,
            Some("setname"),
            Some(ctx.uid), // Exclude sender (we already echoed)
        )
        .await;

        // Extended MONITOR: Notify watchers with extended-monitor + setname
        notify_extended_monitor_watchers(ctx.matrix, &nick, setname_msg, "setname").await;

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
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        // SILENCE [+/-mask]
        let mask_arg = msg.arg(0);

        if mask_arg.is_none() {
            // List silence entries
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;

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

        // Safe: we just checked mask_arg.is_some() above
        let Some(mask_str) = mask_arg else {
            return Err(HandlerError::NeedMoreParams);
        };

        // Check for +/- prefix
        if mask_str.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let (adding, mask) = match mask_str.chars().next() {
            Some('+') => (true, &mask_str[1..]),
            Some('-') => (false, &mask_str[1..]),
            _ => (true, mask_str), // No prefix, treat as add
        };

        if mask.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Update silence list
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            let mut user = user_arc.write().await;

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
