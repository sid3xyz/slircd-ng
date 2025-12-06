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
//! See: <https://modern.ircdocs.horse/ctcp.html>

use super::super::{
    Context, Handler, HandlerError, HandlerResult, server_reply, user_mask_from_state, user_prefix,
};
use super::common::{
    ChannelRouteResult, RouteOptions, is_shunned, route_to_channel, route_to_user,
    send_cannot_send, send_no_such_channel, send_no_such_nick,
};
use crate::db::StoreMessageParams;
use crate::services::route_service_message;
use async_trait::async_trait;
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
            let nick = ctx
                .handshake
                .nick
                .as_ref()
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

        if target.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }
        if text.is_empty() {
            return Err(HandlerError::NoTextToSend);
        }

        // Check for repetition spam (always check)
        if let Some(detector) = &ctx.matrix.spam_detector {
            if let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
                detector.check_message_repetition(&uid_string, text)
            {
                debug!(uid = %uid_string, pattern = %pattern, "Message blocked by spam detector");
                let nick = ctx
                    .handshake
                    .nick
                    .as_ref()
                    .ok_or(HandlerError::NickOrUserMissing)?;
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "Message blocked: repetition detected.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Check for content spam (skip for trusted users)
        let is_trusted = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.modes.oper || user.account.is_some()
        } else {
            false
        };

        if !is_trusted {
            if let Some(detector) = &ctx.matrix.spam_detector {
                if let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
                    detector.check_message(text)
                {
                    debug!(uid = %uid_string, pattern = %pattern, "Message blocked by spam detector");
                    let nick = ctx
                        .handshake
                        .nick
                        .as_ref()
                        .ok_or(HandlerError::NickOrUserMissing)?;
                    let reply = server_reply(
                        &ctx.matrix.server_info.name,
                        Response::ERR_TOOMANYTARGETS,
                        vec![
                            nick.to_string(),
                            target.to_string(),
                            "Message blocked: spam detected.".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }
            }
        }

        // Rate-limit CTCP messages separately to curb floods.
        if slirc_proto::ctcp::Ctcp::is_ctcp(text)
            && !ctx.matrix.rate_limiter.check_ctcp_rate(&uid_string)
        {
            let nick = ctx
                .handshake
                .nick
                .as_ref()
                .ok_or(HandlerError::NickOrUserMissing)?
                .clone();
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_TOOMANYTARGETS,
                vec![
                    nick,
                    "*".to_string(),
                    "You are sending CTCPs too quickly. Please wait.".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Check if this is a service message (NickServ, ChanServ, etc.)
        if route_service_message(ctx.matrix, ctx.uid, &nick, target, text, &ctx.sender).await {
            return Ok(());
        }

        // CTCP messages (VERSION, PING, ACTION, etc.) are just forwarded as PRIVMSG.
        // The IRC server relays them; the target's CLIENT sends NOTICE replies.
        // See: https://modern.ircdocs.horse/ctcp.html

        // Collect client-only tags (those starting with '+') to preserve them
        // Unescape tag values since they come from wire format
        use slirc_proto::message::Tag;
        use std::borrow::Cow;
        let client_tags: Vec<Tag> = msg
            .tags_iter()
            .filter(|(k, _)| k.starts_with('+'))
            .map(|(k, v)| {
                let value = if v.is_empty() {
                    None
                } else {
                    Some(slirc_proto::message::tags::unescape_tag_value(v))
                };
                Tag(Cow::Owned(k.to_string()), value)
            })
            .collect();

        // Build the outgoing message with preserved client tags
        let out_msg = Message {
            tags: if client_tags.is_empty() {
                None
            } else {
                Some(client_tags)
            },
            prefix: Some(user_prefix(&nick, &user_name, &host)),
            command: Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        let opts = RouteOptions {
            send_away_reply: true,
            is_notice: false,
            block_ctcp: true,
            status_prefix: None,
        };

        // STATUSMSG support: @#channel sends to ops, +#channel sends to voiced+
        // Strip status prefix if present and route accordingly
        let (status_prefix, actual_target) = parse_statusmsg(target);
        let routing_target = actual_target.unwrap_or(target);

        if routing_target.is_channel_name() {
            let channel_lower = irc_to_lower(routing_target);


            // If STATUSMSG, route to specific member subset
            if let Some(prefix_char) = status_prefix {
                route_statusmsg(ctx, &channel_lower, target, out_msg, prefix_char).await?;
                debug!(from = %nick, to = %target, prefix = %prefix_char, "PRIVMSG STATUSMSG");
            } else {
                // Regular channel message
                match route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
                    ChannelRouteResult::Sent => {
                        debug!(from = %nick, to = %target, "PRIVMSG to channel");

                        // Store message in history for CHATHISTORY support
                        let msgid = Uuid::new_v4().to_string();
                        let prefix = format!("{}!{}@{}", nick, user_name, host);
                        let account = ctx.handshake.account.as_deref();

                        let params = StoreMessageParams {
                            msgid: &msgid,
                            channel: target,
                            sender_nick: &nick,
                            prefix: &prefix,
                            text,
                            account,
                        };

                        if let Err(e) = ctx.db.history().store_message(params).await {
                            debug!(error = %e, "Failed to store message in history");
                        }
                    }
                    ChannelRouteResult::NoSuchChannel => {
                        send_no_such_channel(ctx, &nick, target).await?;
                    }
                    ChannelRouteResult::BlockedExternal => {
                        send_cannot_send(ctx, &nick, target, "Cannot send to channel (+n)").await?;
                    }
                    ChannelRouteResult::BlockedModerated => {
                        send_cannot_send(ctx, &nick, target, "Cannot send to channel (+m)").await?;
                    }
                    ChannelRouteResult::BlockedRegisteredOnly => {
                        send_cannot_send(ctx, &nick, target, "Cannot send to channel (+r)").await?;
                    }
                    ChannelRouteResult::BlockedCTCP => {
                        send_cannot_send(ctx, &nick, target, "Cannot send CTCP to channel (+C)")
                            .await?;
                    }
                    ChannelRouteResult::BlockedNotice => {
                        // Should not happen for PRIVMSG, but handle anyway
                        send_cannot_send(ctx, &nick, target, "Cannot send NOTICE to channel (+T)")
                            .await?;
                    }
                    ChannelRouteResult::BlockedBanned => {
                        send_cannot_send(ctx, &nick, target, "Cannot send to channel (+b)").await?;
                    }
                }
            }
        } else if route_to_user(ctx, &irc_to_lower(routing_target), out_msg, &opts, &nick).await {
            debug!(from = %nick, to = %target, "PRIVMSG to user");
        } else {
            send_no_such_nick(ctx, &nick, target).await?;
        }

        Ok(())
    }
}

// ============================================================================
// STATUSMSG Helpers
// ============================================================================

/// Parse STATUSMSG prefix from target.
/// Returns (prefix_char, actual_channel_name) if STATUSMSG, otherwise (None, None).
///
/// STATUSMSG allows sending to channel members with specific privileges:
/// - `@#channel` sends to ops
/// - `+#channel` sends to voiced+ (voice or op)
pub(super) fn parse_statusmsg(target: &str) -> (Option<char>, Option<&str>) {
    if target.len() < 2 {
        return (None, None);
    }

    let Some(first_char) = target.chars().next() else {
        return (None, None);
    };
    let rest = &target[first_char.len_utf8()..];

    // Check if it's @#channel or +#channel
    if (first_char == '@' || first_char == '+')
        && rest
            .chars()
            .next()
            .map(|c| c == '#' || c == '&' || c == '+' || c == '!')
            .unwrap_or(false)
    {
        (Some(first_char), Some(rest))
    } else {
        (None, None)
    }
}

/// Route a STATUSMSG to members matching the specified status level.
///
/// - `@`: Send to ops only
/// - `+`: Send to voiced+ (voice or op)
pub(super) async fn route_statusmsg(
    ctx: &Context<'_>,
    channel_lower: &str,
    original_target: &str, // Keep @#chan or +#chan in the message
    msg: Message,
    prefix_char: char,
) -> HandlerResult {
    let opts = RouteOptions {
        send_away_reply: false,
        is_notice: false,
        block_ctcp: false,
        status_prefix: Some(prefix_char),
    };

    match route_to_channel(ctx, channel_lower, msg, &opts).await {
        ChannelRouteResult::NoSuchChannel => {
            let nick = ctx.handshake.nick.as_deref().unwrap_or("*");
            send_no_such_channel(ctx, nick, original_target).await?;
        }
        _ => {}
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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
