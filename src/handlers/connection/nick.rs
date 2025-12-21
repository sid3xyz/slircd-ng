//! NICK command handler for connection registration.
//!
//! # RFC 2812 §3.1.2 - Nick message
//!
//! Used to give user a nickname or change the existing one.
//!
//! **Specification:** [RFC 2812 §3.1.2](https://datatracker.ietf.org/doc/html/rfc2812#section-3.1.2)
//!
//! **Compliance:** 11/11 irctest pass
//!
//! ## Syntax
//! ```text
//! NICK <nickname>
//! ```
//!
//! ## Behavior
//! - Can be used before or after registration
//! - Validates nickname format (length, allowed characters)
//! - Atomically reserves nickname to prevent race conditions
//! - Enforces +N (no nick change) channel mode for registered users
//! - Notifies MONITOR watchers when nickname changes

use super::super::{
    Context, HandlerError, HandlerResult, UniversalHandler, notify_monitors_offline,
    notify_monitors_online,
};
use crate::state::SessionState;
use async_trait::async_trait;
use dashmap::mapref::entry::Entry;
use slirc_proto::{Command, Message, MessageRef, NickExt, Prefix, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tracing::{debug, info};

pub struct NickHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for NickHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // NICK <nickname>
        let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        if !nick.is_valid_nick() {
            return Err(HandlerError::ErroneousNickname(nick.to_string()));
        }

        let nick_lower = irc_to_lower(nick);

        // Check if nick is exactly the same (no-op) - return silently
        if ctx.state.nick().is_some_and(|old| old == nick) {
            return Ok(());
        }

        // Atomically claim nickname (prevents TOCTOU where two clients race between check/insert)
        match ctx.matrix.user_manager.nicks.entry(nick_lower.clone()) {
            Entry::Occupied(entry) => {
                let owner_uid = entry.get();
                if owner_uid != ctx.uid {
                    return Err(HandlerError::NicknameInUse(nick.to_string()));
                }
                // Owner is the same UID; allow case-change or reconnect continuation.
            }
            Entry::Vacant(entry) => {
                entry.insert(ctx.uid.to_string());
            }
        }

        // Check +N (no nick change) on any channel the user is in
        // Only applies to registered (connected) users changing their nick
        if ctx.state.is_registered()
            && let Some(user_arc) = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.clone())
        {
            let user = user_arc.read().await;
            for channel_lower in &user.channels {
                let channel_sender = ctx.matrix.channel_manager.channels.get(channel_lower).map(|c| c.clone());
                if let Some(channel_sender) = channel_sender {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::GetInfo {
                            requester_uid: Some(ctx.uid.to_string()),
                            reply_tx: tx,
                        })
                        .await;

                    if let Ok(info) = rx.await
                        && info
                            .modes
                            .contains(&crate::state::actor::ChannelMode::NoNickChange)
                    {
                        let reply = Response::err_nonickchange(
                            ctx.state.nick_or_star(),
                            nick,
                            &info.name,
                        )
                        .with_prefix(ctx.server_prefix());
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }
            }
        }

        // Save old nick for NICK change notification (before removing from index)
        let old_nick_for_change = if ctx.state.is_registered() {
            ctx.state.nick().map(|s| s.to_string())
        } else {
            None
        };

        // Check if this is a case-only change (qux -> QUX)
        let is_case_only_change = ctx
            .state
            .nick()
            .map(|old| irc_to_lower(old) == nick_lower)
            .unwrap_or(false);

        // Remove old nick from index if changing
        if let Some(old_nick) = ctx.state.nick() {
            let old_nick_lower = irc_to_lower(old_nick);

            // Only notify MONITOR watchers if the lowercase nick is changing
            // (not for case-only changes like qux -> QUX)
            if ctx.state.is_registered() && !is_case_only_change {
                notify_monitors_offline(ctx.matrix, old_nick).await;
            }

            // If the lowercase nick is changing, remove the old mapping (case-only changes keep mapping)
            if old_nick_lower != nick_lower {
                ctx.matrix.user_manager.nicks.remove(&old_nick_lower);
            }
            // Clear any enforcement timer for old nick
            ctx.matrix.user_manager.enforce_timers.remove(ctx.uid);
        }

        // Register new nick
        ctx.matrix
            .user_manager
            .nicks
            .insert(nick_lower.clone(), ctx.uid.to_string());
        ctx.state.set_nick(nick.to_string());

        // Send NICK change message for registered users
        if let Some(old_nick) = old_nick_for_change {
            // Get user info for the prefix and channels
            let (nick_msg, user_channels) = if let Some(user_arc) = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.clone()) {
                let user = user_arc.read().await;
                let msg = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
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
                    prefix: Some(Prefix::new(
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
                    .channel_manager.broadcast_to_channel(channel_lower, nick_msg.clone(), Some(ctx.uid))
                    .await;

                // Update the channel actor's user_nicks map
                let channel_sender = ctx.matrix.channel_manager.channels.get(channel_lower).map(|c| c.clone());
                if let Some(channel_sender) = channel_sender {
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::NickChange {
                            uid: ctx.uid.to_string(),
                            new_nick: nick.to_string(),
                        })
                        .await;
                }
            }

            // Also update the User state with the new nick
            let user_arc = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let mut user = user_arc.write().await;
                user.nick = nick.to_string();
            }

            // Notify observer of user update (Innovation 2)
            ctx.matrix.user_manager.notify_observer(ctx.uid, None).await;
        }

        // Notify MONITOR watchers that new nick is online (only for already-registered users)
        // Skip notification for case-only changes (already computed above)
        if ctx.state.is_registered() && !is_case_only_change {
            // Get user info for the hostmask
            let user_arc = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                notify_monitors_online(ctx.matrix, nick, &user.user, &user.visible_host).await;
            }
        }

        debug!(nick = %nick, uid = %ctx.uid, "Nick set");

        // Check if nick enforcement should be started
        // Only if user is not already identified to an account
        let user_arc = ctx.matrix.user_manager.users.get(ctx.uid).map(|u| u.clone());
        let is_identified = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
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
                    .user_manager
                    .enforce_timers
                    .insert(ctx.uid.to_string(), deadline);

                // Notify user
                let notice = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
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

        // Note: can_register() is only relevant for UnregisteredState.
        // For RegisteredState, we're already registered so this is a no-op.
        // The connection loop handles the registration transition.

        Ok(())
    }
}
