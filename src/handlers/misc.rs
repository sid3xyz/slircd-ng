//! Miscellaneous and operator handlers: AWAY, USERHOST, ISON, INVITE
//!
//! RFC 2812 - Miscellaneous and optional commands

use super::{err_chanoprivsneeded, err_notonchannel, server_reply, Context, Handler, HandlerError, HandlerResult};
use crate::services::chanserv::route_chanserv_message;
use crate::services::nickserv::route_service_message;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, MessageRef, Response};
use tracing::debug;

/// Handler for AWAY command.
///
/// `AWAY [message]`
///
/// Sets or clears away status.
pub struct AwayHandler;

#[async_trait]
impl Handler for AwayHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // AWAY [message]
        let away_msg = msg.arg(0);

        // Update user's away status
        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let _user = user_ref.write().await;
            // Store away status in user (need to add field to User struct)
            // For now, we just send the appropriate response
        }

        if let Some(away_text) = away_msg
            && !away_text.is_empty()
        {
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

/// Handler for USERHOST command.
///
/// `USERHOST nick [nick ...]`
///
/// Returns the user@host for up to 5 nicknames.
pub struct UserhostHandler;

#[async_trait]
impl Handler for UserhostHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // USERHOST <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "USERHOST".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build response (up to 5 nicks)
        let mut replies = Vec::new();
        for target_nick in nicks.iter().take(5) {
            let target_lower = irc_to_lower(target_nick);
            let uid = ctx.matrix.nicks.get(&target_lower);
            let user_ref = uid.as_ref().and_then(|u| ctx.matrix.users.get(u.value()));
            if let Some(user_ref) = user_ref {
                let user = user_ref.read().await;
                // Format: nick[*]=+/-hostname
                // * if oper, + if away, - if not away
                let oper_flag = if user.modes.oper { "*" } else { "" };
                let away_flag = "-"; // TODO: track away status
                replies.push(format!(
                    "{}{}={}{}@{}",
                    user.nick, oper_flag, away_flag, user.user, user.host
                ));
            }
        }

        // RPL_USERHOST (302)
        let reply = server_reply(
            server_name,
            Response::RPL_USERHOST,
            vec![nick.clone(), replies.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for ISON command.
///
/// `ISON nick [nick ...]`
///
/// Returns which of the given nicknames are online.
pub struct IsonHandler;

#[async_trait]
impl Handler for IsonHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // ISON <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "ISON".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Find which nicks are online
        let mut online = Vec::new();
        for target_nick in nicks {
            let target_lower = irc_to_lower(target_nick);
            if ctx.matrix.nicks.contains_key(&target_lower) {
                // Return the nick as the user typed it (case preserved)
                online.push((*target_nick).to_string());
            }
        }

        // RPL_ISON (303)
        let reply = server_reply(
            server_name,
            Response::RPL_ISON,
            vec![nick.clone(), online.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // INVITE <nickname> <channel>
        let target_nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let channel_name = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

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
        if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
            let channel = channel_ref.read().await;

            // Check if user is on channel
            if !channel.is_member(ctx.uid) {
                ctx.sender.send(err_notonchannel(server_name, nick, channel_name)).await?;
                return Ok(());
            }

            // Check if target already on channel
            if channel.is_member(&target_uid) {
                let reply = server_reply(
                    server_name,
                    Response::ERR_USERONCHANNEL,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        channel_name.to_string(),
                        "is already on channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            // If channel is +i, check if user is op
            if channel.modes.invite_only && !channel.is_op(ctx.uid) {
                ctx.sender.send(err_chanoprivsneeded(server_name, nick, channel_name)).await?;
                return Ok(());
            }
        } else {
            // Channel doesn't exist - some servers allow inviting to non-existent channels
            // We'll allow it for now
        }

        // Send INVITE to target
        if let Some(sender) = ctx.matrix.senders.get(&target_uid) {
            let invite_msg = slirc_proto::Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::Nickname(
                    nick.clone(),
                    ctx.handshake.user.clone().unwrap_or_default(),
                    "localhost".to_string(), // TODO: get actual host
                )),
                command: Command::INVITE(target_nick.to_string(), channel_name.to_string()),
            };
            let _ = sender.send(invite_msg).await;
        }

        // RPL_INVITING (341)
        let reply = server_reply(
            server_name,
            Response::RPL_INVITING,
            vec![nick.clone(), target_nick.to_string(), channel_name.to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for KNOCK command.
///
/// `KNOCK channel [message]`
///
/// Requests an invite to a +i channel.
pub struct KnockHandler;

#[async_trait]
impl Handler for KnockHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        use slirc_proto::Prefix;

        // KNOCK <channel> [message]
        let channel_name = match msg.arg(0) {
            Some(c) if !c.is_empty() => c,
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = &ctx.matrix.config.server_name;
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
                    vec![nick, "KNOCK".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };
        let knock_msg = msg.arg(1);

        let server_name = &ctx.matrix.config.server_name;
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
        let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHCHANNEL,
                vec![nick, channel_name.to_string(), "No such channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

            // Check if user is already in channel
        {
            let channel = channel_ref.read().await;
            if channel.is_member(ctx.uid) {
                // ERR_KNOCKONCHAN (714) - already on channel
                let reply = server_reply(
                    server_name,
                    Response::ERR_KNOCKONCHAN,
                    vec![nick, channel_name.to_string(), "You're already on that channel".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }            // Check if channel is +i (invite only)
            if !channel.modes.invite_only {
                // ERR_CHANOPEN (713) - channel not invite-only
                let reply = server_reply(
                    server_name,
                    Response::ERR_CHANOPEN,
                    vec![nick.clone(), channel_name.to_string(), "Channel is open, just join it".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Build KNOCK notification for channel ops
        let knock_text = knock_msg.map(|s| s.to_string()).unwrap_or_else(|| "has asked for an invite".to_string());
        let knock_notice = slirc_proto::Message {
            tags: None,
            prefix: Some(Prefix::Nickname(nick.clone(), user, host)),
            command: Command::KNOCK(channel_name.to_string(), Some(knock_text)),
        };

        // Send to channel operators
        {
            let channel = channel_ref.read().await;
            for (member_uid, modes) in &channel.members {
                if modes.op
                    && let Some(sender) = ctx.matrix.senders.get(member_uid)
                {
                    let _ = sender.send(knock_notice.clone()).await;
                }
            }
        }

        // RPL_KNOCKDLVR (711) - knock delivered
        let reply = server_reply(
            server_name,
            Response::RPL_KNOCKDLVR,
            vec![nick, channel_name.to_string(), "Your knock has been delivered".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for NS (NickServ alias) command.
///
/// `NS <command> [args]`
///
/// Shortcut for PRIVMSG NickServ.
pub struct NsHandler;

#[async_trait]
impl Handler for NsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // Join all args into the command text
        let text = msg.args().join(" ");

        if text.is_empty() {
            // Show help
            route_service_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                "NickServ",
                "HELP",
                ctx.sender,
            ).await;
        } else {
            // Route to NickServ
            route_service_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                "NickServ",
                &text,
                ctx.sender,
            ).await;
        }

        Ok(())
    }
}

/// Handler for CS (ChanServ alias) command.
///
/// `CS <command> [args]`
///
/// Shortcut for PRIVMSG ChanServ.
pub struct CsHandler;

#[async_trait]
impl Handler for CsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // Join all args into the command text
        let text = msg.args().join(" ");

        if text.is_empty() {
            // Show help
            route_chanserv_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                "ChanServ",
                "HELP",
                ctx.sender,
            ).await;
        } else {
            // Route to ChanServ
            route_chanserv_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                "ChanServ",
                &text,
                ctx.sender,
            ).await;
        }

        Ok(())
    }
}
