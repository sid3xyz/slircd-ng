//! PRIVMSG command handler.
//!
//! Handles private messages to users and channels, with support for CTCP.
//!
//! ## CTCP Handling (per RFC/IRCv3)
//!
//! CTCP messages are simply PRIVMSG/NOTICE with special `\x01...\x01` delimiters.
//! The IRC server RELAYS these messages to the target; it does NOT intercept or
//! respond to CTCP requests. The target CLIENT is responsible for responding.
//!
//! - CTCP requests are sent via PRIVMSG
//! - CTCP replies are sent via NOTICE
//! - The server only enforces +C channel mode (no CTCP except ACTION)
//!
//! See: https://modern.ircdocs.horse/ctcp.html

use super::common::{
    is_shunned, route_to_channel, route_to_user, send_cannot_send,
    send_no_such_channel, send_no_such_nick, ChannelRouteResult, RouteOptions,
};
use super::super::{Context, Handler, HandlerError, HandlerResult, server_reply, user_prefix};
use crate::db::StoreMessageParams;
use crate::services::route_service_message;
use async_trait::async_trait;
use slirc_proto::ctcp::{Ctcp, CtcpKind};
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Response, irc_to_lower};
use tracing::debug;
use uuid::Uuid;

// ============================================================================
// PRIVMSG Handler
// ============================================================================

/// Handler for PRIVMSG command.
pub struct PrivmsgHandler;

#[async_trait]
impl Handler for PrivmsgHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // Check message rate limit
        let uid_string = ctx.uid.to_string();
        if !ctx.matrix.rate_limiter.check_message_rate(&uid_string) {
            let nick = ctx.handshake.nick.as_ref()
                .ok_or(HandlerError::NickOrUserMissing)?;
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_TOOMANYTARGETS,
                vec![
                    nick.to_string(),
                    "*".to_string(),
                    "You are sending messages too quickly. Please wait.".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // PRIVMSG <target> <text>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx
            .handshake
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Check if this is a service message (NickServ, ChanServ, etc.)
        if route_service_message(ctx.matrix, ctx.db, ctx.uid, nick, target, text, ctx.sender)
            .await
        {
            return Ok(());
        }

        // CTCP messages (VERSION, PING, ACTION, etc.) are just forwarded as PRIVMSG.
        // The IRC server relays them; the target's CLIENT sends NOTICE replies.
        // See: https://modern.ircdocs.horse/ctcp.html

        // Build the outgoing message
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        let opts = RouteOptions {
            check_moderated: true,
            send_away_reply: true,
            is_notice: false,
            strip_colors: true,
            block_ctcp: true,
        };

        if target.is_channel_name() {
            let channel_lower = irc_to_lower(target);

            // Check +C mode (no CTCP except ACTION) for channel messages
            if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel_ref.read().await;
                if channel.modes.no_ctcp
                    && Ctcp::is_ctcp(text)
                    && let Some(ctcp) = Ctcp::parse(text)
                    && !matches!(ctcp.kind, CtcpKind::Action)
                {
                    send_cannot_send(ctx, nick, target, "Cannot send CTCP to channel (+C)").await?;
                    return Ok(());
                }
            }

            match route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
                ChannelRouteResult::Sent => {
                    debug!(from = %nick, to = %target, "PRIVMSG to channel");

                    // Store message in history for CHATHISTORY support
                    let msgid = Uuid::new_v4().to_string();
                    let prefix = format!("{}!{}@localhost", nick, user_name);
                    let account = ctx.handshake.account.as_deref();

                    let params = StoreMessageParams {
                        msgid: &msgid,
                        channel: target,
                        sender_nick: nick,
                        prefix: &prefix,
                        text,
                        account,
                    };

                    if let Err(e) = ctx.db.history().store_message(params).await {
                        debug!(error = %e, "Failed to store message in history");
                    }
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+n)").await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+m)").await?;
                }
                ChannelRouteResult::BlockedSpam => {
                    send_cannot_send(ctx, nick, target, "Message rejected as spam").await?;
                }
                ChannelRouteResult::BlockedRegisteredOnly => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+r)").await?;
                }
                ChannelRouteResult::BlockedCTCP => {
                    send_cannot_send(ctx, nick, target, "Cannot send CTCP to channel (+C)").await?;
                }
                ChannelRouteResult::BlockedNotice => {
                    // Should not happen for PRIVMSG, but handle anyway
                    send_cannot_send(ctx, nick, target, "Cannot send NOTICE to channel (+T)").await?;
                }
            }
        } else if route_to_user(ctx, &irc_to_lower(target), out_msg, &opts, nick).await {
            debug!(from = %nick, to = %target, "PRIVMSG to user");
        } else {
            send_no_such_nick(ctx, nick, target).await?;
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::ctcp::{Ctcp, CtcpKind};

    #[test]
    fn test_ctcp_parsing() {
        // Verify slirc_proto's CTCP parsing works as expected
        let version = Ctcp::parse("\x01VERSION\x01");
        assert!(version.is_some());
        assert!(matches!(version.unwrap().kind, CtcpKind::Version));

        let ping = Ctcp::parse("\x01PING 1234567890\x01");
        assert!(ping.is_some());
        let ping = ping.unwrap();
        assert!(matches!(ping.kind, CtcpKind::Ping));
        assert_eq!(ping.params, Some("1234567890"));

        let time = Ctcp::parse("\x01TIME\x01");
        assert!(time.is_some());
        assert!(matches!(time.unwrap().kind, CtcpKind::Time));

        let clientinfo = Ctcp::parse("\x01CLIENTINFO\x01");
        assert!(clientinfo.is_some());
        assert!(matches!(clientinfo.unwrap().kind, CtcpKind::Clientinfo));

        let action = Ctcp::parse("\x01ACTION waves\x01");
        assert!(action.is_some());
        let action = action.unwrap();
        assert!(matches!(action.kind, CtcpKind::Action));
        assert_eq!(action.params, Some("waves"));
    }

    #[test]
    fn test_ctcp_is_ctcp() {
        assert!(Ctcp::is_ctcp("\x01VERSION\x01"));
        assert!(Ctcp::is_ctcp("\x01ACTION test\x01"));
        assert!(!Ctcp::is_ctcp("regular message"));
        // slirc_proto is lenient: strings starting with \x01 are considered CTCP
        // and parse() accepts messages without trailing \x01 (real-world tolerance)
        assert!(Ctcp::is_ctcp("\x01incomplete"));
        assert!(Ctcp::parse("\x01incomplete").is_some()); // Lenient parsing
    }
}
