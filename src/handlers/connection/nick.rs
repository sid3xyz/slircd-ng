//! NICK command handler for connection registration.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, notify_monitors_offline, notify_monitors_online,
    server_reply,
};
use super::welcome::send_welcome_burst;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, NickExt, Prefix, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Handler for NICK command.
pub struct NickHandler;

#[async_trait]
impl Handler for NickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // NICK <nickname>
        let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        if !nick.is_valid_nick() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ERRONEOUSNICKNAME,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    nick.to_string(),
                    "Erroneous nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let nick_lower = irc_to_lower(nick);

        // Check if nick is exactly the same (no-op) - return silently
        if ctx.handshake.nick.as_ref().is_some_and(|old| old == nick) {
            return Ok(());
        }

        // Check if nick is in use
        if let Some(existing_uid) = ctx.matrix.nicks.get(&nick_lower)
            && existing_uid.value() != ctx.uid
        {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NICKNAMEINUSE,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    nick.to_string(),
                    "Nickname is already in use".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Check +N (no nick change) on any channel the user is in
        // Only applies to registered (connected) users changing their nick
        if ctx.handshake.registered
            && let Some(user_ref) = ctx.matrix.users.get(ctx.uid)
        {
            let user = user_ref.read().await;
            for channel_lower in &user.channels {
                if let Some(channel_sender) = ctx.matrix.channels.get(channel_lower) {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = channel_sender.send(crate::state::actor::ChannelEvent::GetInfo {
                        requester_uid: Some(ctx.uid.to_string()),
                        reply_tx: tx
                    }).await;

                    if let Ok(info) = rx.await {
                        if info.modes.contains(&crate::state::actor::ChannelMode::NoNickChange) {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_NONICKCHANGE,
                                vec![
                                    ctx.handshake
                                        .nick
                                        .clone()
                                        .unwrap_or_else(|| "*".to_string()),
                                    info.name.clone(),
                                    "Cannot change nickname while in this channel (+N)".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Save old nick for NICK change notification (before removing from index)
        let old_nick_for_change = if ctx.handshake.registered {
            ctx.handshake.nick.clone()
        } else {
            None
        };

        // Check if this is a case-only change (qux -> QUX)
        let is_case_only_change = ctx
            .handshake
            .nick
            .as_ref()
            .map(|old| irc_to_lower(old) == nick_lower)
            .unwrap_or(false);

        // Remove old nick from index if changing
        if let Some(old_nick) = &ctx.handshake.nick {
            let old_nick_lower = irc_to_lower(old_nick);

            // Only notify MONITOR watchers if the lowercase nick is changing
            // (not for case-only changes like qux -> QUX)
            if ctx.handshake.registered && !is_case_only_change {
                notify_monitors_offline(ctx.matrix, old_nick).await;
            }

            ctx.matrix.nicks.remove(&old_nick_lower);
            // Clear any enforcement timer for old nick
            ctx.matrix.enforce_timers.remove(ctx.uid);
        }

        // Register new nick
        ctx.matrix
            .nicks
            .insert(nick_lower.clone(), ctx.uid.to_string());
        ctx.handshake.nick = Some(nick.to_string());

        // Send NICK change message for registered users
        if let Some(old_nick) = old_nick_for_change {
            // Get user info for the prefix and channels
            let (nick_msg, user_channels) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                let msg = Message {
                    tags: None,
                    prefix: Some(Prefix::Nickname(
                        old_nick.clone(),
                        user.user.clone(),
                        user.visible_host.clone(),
                    )),
                    command: Command::NICK(nick.to_string()),
                };
                let channels = user.channels.clone();
                (msg, channels)
            } else {
                // Fallback without full user info
                let msg = Message {
                    tags: None,
                    prefix: Some(Prefix::Nickname(
                        old_nick.clone(),
                        "user".to_string(),
                        "host".to_string(),
                    )),
                    command: Command::NICK(nick.to_string()),
                };
                (msg, std::collections::HashSet::new())
            };

            // Send to the user themselves with label (IRCv3 labeled-response)
            let labeled_nick_msg = super::super::with_label(nick_msg.clone(), ctx.label.as_deref());
            ctx.sender.send(labeled_nick_msg).await?;

            // Broadcast to all channels the user is in (including case-only changes)
            for channel_lower in &user_channels {
                ctx.matrix
                    .broadcast_to_channel(channel_lower, nick_msg.clone(), Some(ctx.uid))
                    .await;

                // Update the channel actor's user_nicks map
                if let Some(channel_sender) = ctx.matrix.channels.get(channel_lower) {
                    let _ = channel_sender.send(crate::state::actor::ChannelEvent::NickChange {
                        uid: ctx.uid.to_string(),
                        old_nick: old_nick.clone(),
                        new_nick: nick.to_string(),
                    }).await;
                }
            }

            // Also update the User state with the new nick
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let mut user = user_ref.write().await;
                user.nick = nick.to_string();
            }
        }

        // Notify MONITOR watchers that new nick is online (only for already-registered users)
        // Skip notification for case-only changes (already computed above)
        if ctx.handshake.registered && !is_case_only_change {
            // Get user info for the hostmask
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                notify_monitors_online(ctx.matrix, nick, &user.user, &user.visible_host).await;
            }
        }

        debug!(nick = %nick, uid = %ctx.uid, "Nick set");

        // Check if nick enforcement should be started
        // Only if user is not already identified to an account
        let is_identified = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
            let user = user.read().await;
            user.modes.registered
        } else {
            false
        };

        if !is_identified {
            // Check if this nick is registered with ENFORCE enabled
            if let Ok(Some(account)) = ctx.db.accounts().find_by_nickname(nick).await
                && account.enforce
            {
                // Start 60 second timer
                let deadline = Instant::now() + Duration::from_secs(60);
                ctx.matrix
                    .enforce_timers
                    .insert(ctx.uid.to_string(), deadline);

                // Notify user
                let notice = Message {
                    tags: None,
                    prefix: Some(Prefix::Nickname(
                        "NickServ".to_string(),
                        "NickServ".to_string(),
                        "services.".to_string(),
                    )),
                    command: Command::NOTICE(
                        nick.to_string(),
                        "This nickname is registered. Please identify via \x02/msg NickServ IDENTIFY <password>\x02 within 60 seconds.".to_string(),
                    ),
                };
                let _ = ctx.sender.send(notice).await;
                info!(nick = %nick, uid = %ctx.uid, "Nick enforcement timer started");
            }
        }

        // Check if we can complete registration
        if ctx.handshake.can_register() {
            send_welcome_burst(ctx).await?;
        }

        Ok(())
    }
}
