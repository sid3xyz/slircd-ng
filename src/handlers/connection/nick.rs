//! NICK command handler for connection registration.

use super::super::{Context, Handler, HandlerError, HandlerResult, notify_monitors_offline, notify_monitors_online, server_reply};
use super::welcome::send_welcome_burst;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Validates an IRC nickname per RFC 2812.
/// First char: letter or special [\]^_`{|}
/// Rest: letter, digit, special, or hyphen
fn is_valid_nick(nick: &str) -> bool {
    if nick.is_empty() || nick.len() > 30 {
        return false;
    }

    let is_special = |c: char| matches!(c, '[' | ']' | '\\' | '`' | '_' | '^' | '{' | '|' | '}');

    let mut chars = nick.chars();
    let first = chars.next().unwrap();

    // First char: letter or special
    if !first.is_ascii_alphabetic() && !is_special(first) {
        return false;
    }

    // Rest: letter, digit, special, or hyphen
    chars.all(|c| c.is_ascii_alphanumeric() || is_special(c) || c == '-')
}

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

        if !is_valid_nick(nick) {
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
                if let Some(channel_ref) = ctx.matrix.channels.get(channel_lower) {
                    let channel = channel_ref.read().await;
                    if channel.modes.no_nick_change {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::ERR_NONICKCHANGE,
                            vec![
                                ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                                channel.name.clone(),
                                "Cannot change nickname while in this channel (+N)".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }
            }
        }

        // Remove old nick from index if changing
        if let Some(old_nick) = &ctx.handshake.nick {
            // Notify MONITOR watchers that old nick is going offline
            if ctx.handshake.registered {
                notify_monitors_offline(ctx.matrix, old_nick).await;
            }

            let old_nick_lower = irc_to_lower(old_nick);
            ctx.matrix.nicks.remove(&old_nick_lower);
            // Clear any enforcement timer for old nick
            ctx.matrix.enforce_timers.remove(ctx.uid);
        }

        // Register new nick
        ctx.matrix
            .nicks
            .insert(nick_lower.clone(), ctx.uid.to_string());
        ctx.handshake.nick = Some(nick.to_string());

        // Notify MONITOR watchers that new nick is online (only for already-registered users)
        if ctx.handshake.registered {
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
