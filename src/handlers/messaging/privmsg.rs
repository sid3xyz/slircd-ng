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
    Context, HandlerError, HandlerResult, PostRegHandler, user_mask_from_state, user_prefix,
};
use crate::state::RegisteredState;
use super::common::{
    ChannelRouteResult, RouteOptions, route_to_channel, route_to_user, send_cannot_send,
    send_no_such_channel, send_no_such_nick,
};
use super::validation::{ErrorStrategy, validate_message_send};
use crate::db::StoreMessageParams;
use crate::services::route_service_message;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, irc_to_lower};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use tracing::debug;
use uuid::Uuid;

// ============================================================================
// PRIVMSG Handler
// ============================================================================

/// Handler for PRIVMSG command.
pub struct PrivmsgHandler;

#[async_trait]
impl PostRegHandler for PrivmsgHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // PRIVMSG <target> <text>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }
        if text.is_empty() {
            return Err(HandlerError::NoTextToSend);
        }

        // Use shared validation (shun, rate limiting, spam detection)
        validate_message_send(ctx, target, text, ErrorStrategy::SendError).await?;

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

        // Collect client-only tags (those starting with '+') AND the label tag to preserve them
        // The label tag is needed for labeled-response echoes back to the sender
        // Unescape tag values since they come from wire format
        use slirc_proto::message::Tag;
        use std::borrow::Cow;
        let preserved_tags: Vec<Tag> = msg
            .tags_iter()
            .filter(|(k, _)| k.starts_with('+') || *k == "label")
            .map(|(k, v)| {
                let value = if v.is_empty() {
                    None
                } else {
                    Some(slirc_proto::message::tags::unescape_tag_value(v))
                };
                Tag(Cow::Owned(k.to_string()), value)
            })
            .collect();

        // Generate timestamp once for consistency between live message and history
        // Truncate to milliseconds to match IRCv3 server-time precision and avoid
        // discrepancies between stored time and time tag.
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let millis = duration.as_millis() as i64;
        let nanotime = millis * 1_000_000;

        // Re-create DateTime from truncated millis to ensure consistency
        let dt = DateTime::<Utc>::from_timestamp(millis / 1000, (millis % 1000) as u32 * 1_000_000).unwrap_or_default();
        let timestamp_iso = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let msgid = Uuid::new_v4().to_string();

        // Build the outgoing message with preserved tags (client tags + label)
        let out_msg = Message {
            tags: if preserved_tags.is_empty() {
                None
            } else {
                Some(preserved_tags)
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
                route_statusmsg(ctx, &channel_lower, target, out_msg, prefix_char, Some(timestamp_iso.clone()), Some(msgid.clone())).await?;
                debug!(from = %nick, to = %target, prefix = %prefix_char, "PRIVMSG STATUSMSG");
                // Suppress ACK for echo-message with labels (echo IS the response)
                if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                    ctx.suppress_labeled_ack = true;
                }
            } else {
                // Regular channel message
                match route_to_channel(ctx, &channel_lower, out_msg, &opts, Some(timestamp_iso.clone()), Some(msgid.clone())).await {
                    ChannelRouteResult::Sent => {
                        debug!(from = %nick, to = %target, "PRIVMSG to channel");

                        // If user has echo-message, suppress ACK - the echo IS the response
                        // This is for labeled-response: when echo-message echoes back,
                        // we don't need a separate ACK
                        if ctx.label.is_some()
                            && ctx.state.capabilities.contains("echo-message")
                        {
                            ctx.suppress_labeled_ack = true;
                        }

                        // Store message in history for CHATHISTORY support
                        let prefix = format!("{}!{}@{}", nick, user_name, host);
                        let account = ctx.state.account.as_deref();

                        let params = StoreMessageParams {
                            msgid: &msgid,
                            channel: target,
                            sender_nick: &nick,
                            prefix: &prefix,
                            text,
                            account,
                            target_account: None,
                            nanotime: Some(nanotime),
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
        } else if route_to_user(ctx, &irc_to_lower(routing_target), out_msg, &opts, &nick, Some(timestamp_iso.clone()), Some(msgid.clone())).await {
            debug!(from = %nick, to = %target, "PRIVMSG to user");

            // Store message in history for CHATHISTORY support (DMs)
            let prefix = format!("{}!{}@{}", nick, user_name, host);
            let account = ctx.state.account.as_deref();

            // Lookup target account
            let target_lower = irc_to_lower(routing_target);
            let target_account = if let Some(uid_ref) = ctx.matrix.nicks.get(&target_lower) {
                 let uid = uid_ref.value();
                 if let Some(user_ref) = ctx.matrix.users.get(uid) {
                     let user = user_ref.read().await;
                     user.account.clone()
                 } else {
                     None
                 }
            } else {
                 None
            };

            let params = StoreMessageParams {
                msgid: &msgid,
                channel: target, // For DMs, channel is the recipient nick
                sender_nick: &nick,
                prefix: &prefix,
                text,
                account,
                target_account: target_account.as_deref(),
                nanotime: Some(nanotime),
            };

            if let Err(e) = ctx.db.history().store_message(params).await {
                debug!(error = %e, "Failed to store DM in history");
            }
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
/// - `~#channel` sends to owners
/// - `&#channel` sends to admins+ (admin or owner)
/// - `@#channel` sends to ops+ (op, admin, or owner)
/// - `%#channel` sends to halfops+ (halfop, op, admin, or owner)
/// - `+#channel` sends to voiced+ (voice, halfop, op, admin, or owner)
pub(super) fn parse_statusmsg(target: &str) -> (Option<char>, Option<&str>) {
    if target.len() < 2 {
        return (None, None);
    }

    let Some(first_char) = target.chars().next() else {
        return (None, None);
    };
    let rest = &target[first_char.len_utf8()..];

    // Check for valid STATUSMSG prefixes followed by a channel character
    if matches!(first_char, '~' | '&' | '@' | '%' | '+')
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
    ctx: &Context<'_, crate::state::RegisteredState>,
    channel_lower: &str,
    original_target: &str, // Keep @#chan or +#chan in the message
    msg: Message,
    prefix_char: char,
    timestamp: Option<String>,
    msgid: Option<String>,
) -> HandlerResult {
    let opts = RouteOptions {
        send_away_reply: false,
        is_notice: false,
        block_ctcp: false,
        status_prefix: Some(prefix_char),
    };

    if route_to_channel(ctx, channel_lower, msg, &opts, timestamp, msgid).await == ChannelRouteResult::NoSuchChannel {
        let nick = &ctx.state.nick;
        send_no_such_channel(ctx, nick, original_target).await?;
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
