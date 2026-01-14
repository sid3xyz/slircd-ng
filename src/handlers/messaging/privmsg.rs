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

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, user_prefix};
use super::common::{
    ChannelRouteResult, RouteMeta, RouteOptions, SenderSnapshot, UserRouteResult,
    route_to_channel_with_snapshot, route_to_user_with_snapshot, send_cannot_send,
    send_no_such_channel,
};
use super::errors::*;
use super::validation::{ErrorStrategy, validate_message_send};
use crate::history::types::MessageTag as HistoryTag;
use crate::history::{MessageEnvelope, StoredMessage};
use crate::services::route_service_message;
use crate::state::RegisteredState;
use crate::state::dashmap_ext::DashMapExt;
use crate::telemetry::spans;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use slirc_proto::{ChannelExt, Command, Message, MessageRef, irc_to_lower};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::Instrument;
use tracing::debug;
use uuid::Uuid;

// ============================================================================
// Prepared Message (DRY)
// ============================================================================

/// Pre-computed message data for routing.
///
/// Consolidates all the computed values needed for message routing and storage
/// to avoid redundant calculations across channel and user routing paths.
struct PreparedMessage {
    /// The IRC message ready to send (with tags and prefix)
    out_msg: Message,
    /// Preserved client-only tags for history storage
    history_tags: Option<Vec<HistoryTag>>,
    /// ISO 8601 timestamp for server-time
    timestamp_iso: String,
    /// Unique message ID
    msgid: String,
    /// Timestamp in nanoseconds (for history ordering)
    nanotime: i64,
    /// User prefix string for history
    prefix: String,
}

/// Prepare message metadata once for routing to multiple targets.
fn prepare_message(
    msg: &MessageRef<'_>,
    target: &str,
    text: &str,
    snapshot: &SenderSnapshot,
) -> PreparedMessage {
    // Collect client-only tags (those starting with '+') AND the label tag
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

    // Generate timestamp (truncated to milliseconds for IRCv3 server-time precision)
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let millis = duration.as_millis() as i64;
    let nanotime = millis * 1_000_000;

    let dt = DateTime::<Utc>::from_timestamp(millis / 1000, (millis % 1000) as u32 * 1_000_000)
        .unwrap_or_default();
    let timestamp_iso = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let msgid = Uuid::new_v4().to_string();

    // Prepare tags for history (before preserved_tags is moved)
    let history_tags: Option<Vec<HistoryTag>> = if preserved_tags.is_empty() {
        None
    } else {
        Some(
            preserved_tags
                .iter()
                .map(|t| HistoryTag {
                    key: t.0.to_string(),
                    value: t.1.as_ref().map(|v| v.to_string()),
                })
                .collect(),
        )
    };

    // Build the outgoing message with preserved tags
    let out_msg = Message {
        tags: if preserved_tags.is_empty() {
            None
        } else {
            Some(preserved_tags)
        },
        prefix: Some(user_prefix(
            &snapshot.nick,
            &snapshot.user,
            &snapshot.visible_host,
        )),
        command: Command::PRIVMSG(target.to_string(), text.to_string()),
    };

    let prefix = format!(
        "{}!{}@{}",
        snapshot.nick, snapshot.user, snapshot.visible_host
    );

    PreparedMessage {
        out_msg,
        history_tags,
        timestamp_iso,
        msgid,
        nanotime,
        prefix,
    }
}

/// Create a StoredMessage for history.
fn create_stored_message(
    prepared: &PreparedMessage,
    target: &str,
    text: &str,
    snapshot: &SenderSnapshot,
    account: &Option<String>,
) -> StoredMessage {
    StoredMessage {
        msgid: prepared.msgid.clone(),
        target: irc_to_lower(target),
        sender: snapshot.nick.clone(),
        envelope: MessageEnvelope {
            command: "PRIVMSG".to_string(),
            prefix: prepared.prefix.clone(),
            target: target.to_string(),
            text: text.to_string(),
            tags: prepared.history_tags.clone(),
        },
        nanotime: prepared.nanotime,
        account: account.clone(),
    }
}

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
        let targets_raw = msg.arg(0);
        let span = spans::command("PRIVMSG", ctx.uid, targets_raw);

        async move {
            // PRIVMSG <target> <text>
            let targets = targets_raw.ok_or(HandlerError::NeedMoreParams)?;
            let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

            if targets.is_empty() {
                return Err(HandlerError::NeedMoreParams);
            }
            if text.is_empty() {
                return Err(HandlerError::NoTextToSend);
            }

            // Build sender snapshot once (eliminates redundant user reads across validation + routing)
            let snapshot = SenderSnapshot::build(ctx)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;

            // Split comma-separated targets (RFC 2812 section 3.3.1)
            let target_list: Vec<&str> = targets.split(',').map(|s| s.trim()).collect();

            // Process each target individually
            for target in target_list {
                if target.is_empty() {
                    continue;
                }

                // Use shared validation (shun, rate limiting, spam detection)
                validate_message_send(ctx, target, text, ErrorStrategy::SendError, &snapshot)
                    .await?;

                // Check if this is a service message (NickServ, ChanServ, etc.)
                if route_service_message(
                    ctx.matrix,
                    ctx.uid,
                    &snapshot.nick,
                    target,
                    text,
                    &ctx.sender,
                )
                .await
                {
                    continue;
                }

                // Prepare message with tags, timestamp, msgid (computed once per target)
                let prepared = prepare_message(msg, target, text, &snapshot);

                // STATUSMSG support: @#channel sends to ops, +#channel sends to voiced+
                let (status_prefix, actual_target) = parse_statusmsg(target);
                let routing_target = actual_target.unwrap_or(target);

                if routing_target.is_channel_name() {
                    route_to_channel_target(ctx, target, text, &snapshot, &prepared, status_prefix)
                        .await?;
                } else {
                    route_to_user_target(ctx, target, text, &snapshot, &prepared, routing_target)
                        .await?;
                }
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}

// ============================================================================
// Routing Helpers
// ============================================================================

/// Route PRIVMSG to a channel target.
async fn route_to_channel_target(
    ctx: &mut Context<'_, RegisteredState>,
    target: &str,
    text: &str,
    snapshot: &SenderSnapshot,
    prepared: &PreparedMessage,
    status_prefix: Option<char>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(target.trim_start_matches(['~', '&', '@', '%', '+']));

    if let Some(prefix_char) = status_prefix {
        // STATUSMSG - route to specific member subset
        route_statusmsg(
            ctx,
            StatusMsgParams {
                channel_lower: &channel_lower,
                original_target: target,
                msg: prepared.out_msg.clone(),
                prefix_char,
                timestamp: Some(prepared.timestamp_iso.clone()),
                msgid: Some(prepared.msgid.clone()),
                snapshot,
            },
        )
        .await?;
        debug!(from = %snapshot.nick, to = %target, prefix = %prefix_char, "PRIVMSG STATUSMSG");
        suppress_labeled_ack_if_echo(ctx);
    } else {
        // Regular channel message
        let opts = RouteOptions {
            send_away_reply: true,
            status_prefix: None,
        };

        match route_to_channel_with_snapshot(
            ctx,
            &channel_lower,
            prepared.out_msg.clone(),
            &opts,
            RouteMeta {
                timestamp: Some(prepared.timestamp_iso.clone()),
                msgid: Some(prepared.msgid.clone()),
                override_nick: None,
                relaymsg_sender_nick: None,
            },
            snapshot,
        )
        .await
        {
            ChannelRouteResult::Sent => {
                debug!(from = %snapshot.nick, to = %target, "PRIVMSG to channel");
                suppress_labeled_ack_if_echo(ctx);

                // Store message in history for CHATHISTORY support
                let stored_msg =
                    create_stored_message(prepared, target, text, snapshot, &ctx.state.account);
                if let Err(e) = ctx
                    .matrix
                    .service_manager
                    .history
                    .store(target, stored_msg)
                    .await
                {
                    debug!(error = %e, "Failed to store message in history");
                }
            }
            ChannelRouteResult::NoSuchChannel => {
                send_no_such_channel(ctx, &snapshot.nick, target).await?;
            }
            ChannelRouteResult::BlockedExternal => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_NOT_IN_CHANNEL).await?;
            }
            ChannelRouteResult::BlockedModerated => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_MODERATED).await?;
            }
            ChannelRouteResult::BlockedRegisteredOnly => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_REGISTERED_ONLY).await?;
            }
            ChannelRouteResult::BlockedRegisteredSpeak => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_REGISTERED_SPEAK).await?;
            }
            ChannelRouteResult::BlockedCTCP => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_CTCP).await?;
            }
            ChannelRouteResult::BlockedNotice => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_NOTICE).await?;
            }
            ChannelRouteResult::BlockedBanned => {
                send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_BANNED).await?;
            }
        }
    }
    Ok(())
}

/// Route PRIVMSG to a user target.
async fn route_to_user_target(
    ctx: &mut Context<'_, RegisteredState>,
    target: &str,
    text: &str,
    snapshot: &SenderSnapshot,
    prepared: &PreparedMessage,
    routing_target: &str,
) -> HandlerResult {
    let opts = RouteOptions {
        send_away_reply: true,
        status_prefix: None,
    };

    let target_lower = irc_to_lower(routing_target);

    // Auto-Accept Logic: Add target to sender's accept list
    // This allows the target to reply even if sender has +R
    if let Some(user_arc) = ctx.matrix.user_manager.users.get(ctx.uid) {
        let mut user = user_arc.write().await;
        user.accept_list.insert(target_lower.clone());
    }

    match route_to_user_with_snapshot(
        ctx,
        &target_lower,
        prepared.out_msg.clone(),
        &opts,
        Some(prepared.timestamp_iso.clone()),
        Some(prepared.msgid.clone()),
        snapshot,
    )
    .await
    {
        UserRouteResult::Sent => {
            debug!(from = %snapshot.nick, to = %target, "PRIVMSG to user");

            // Store DM in history with canonical key
            let stored_msg =
                create_stored_message(prepared, target, text, snapshot, &ctx.state.account);
            let dm_key = compute_dm_key(ctx, &target_lower, snapshot).await;

            if let Err(e) = ctx
                .matrix
                .service_manager
                .history
                .store(&dm_key, stored_msg)
                .await
            {
                debug!(error = %e, "Failed to store DM");
            }
        }
        UserRouteResult::NoSuchNick => {
            crate::handlers::send_no_such_nick(ctx, "PRIVMSG", target).await?;
        }
        UserRouteResult::BlockedRegisteredOnly => {
            let reply = slirc_proto::Response::err_needreggednick(&snapshot.nick, target);
            ctx.sender.send(reply).await?;
        }
        UserRouteResult::BlockedSilence | UserRouteResult::BlockedCtcp => {
            // Silent drop
        }
    }
    Ok(())
}

/// Compute the canonical DM key for history storage.
/// Format: dm:user1:user2 (sorted alphabetically)
/// Uses account name if available, otherwise nick, with prefix to avoid collisions.
async fn compute_dm_key(
    ctx: &Context<'_, RegisteredState>,
    target_lower: &str,
    snapshot: &SenderSnapshot,
) -> String {
    let sender_key_part = if let Some(acct) = &snapshot.account {
        format!("a:{}", irc_to_lower(acct))
    } else {
        format!("u:{}", irc_to_lower(&snapshot.nick))
    };

    let target_account = if let Some(uid) = ctx.matrix.user_manager.nicks.get_cloned(target_lower) {
        if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(&uid) {
            let u = user_arc.read().await;
            u.account.clone()
        } else {
            None
        }
    } else {
        None
    };

    let target_key_part = if let Some(acct) = target_account {
        format!("a:{}", irc_to_lower(&acct))
    } else {
        format!("u:{}", target_lower)
    };

    let mut users = [sender_key_part, target_key_part];
    users.sort();
    format!("dm:{}:{}", users[0], users[1])
}

/// Suppress labeled ACK if echo-message is enabled.
fn suppress_labeled_ack_if_echo(ctx: &mut Context<'_, RegisteredState>) {
    if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
        ctx.suppress_labeled_ack = true;
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

/// Parameters for STATUSMSG routing.
pub(super) struct StatusMsgParams<'a> {
    pub channel_lower: &'a str,
    pub original_target: &'a str,
    pub msg: Message,
    pub prefix_char: char,
    pub timestamp: Option<String>,
    pub msgid: Option<String>,
    pub snapshot: &'a SenderSnapshot,
}

/// Route a STATUSMSG to members matching the specified status level.
///
/// - `@`: Send to ops only
/// - `+`: Send to voiced+ (voice or op)
pub(super) async fn route_statusmsg(
    ctx: &Context<'_, crate::state::RegisteredState>,
    params: StatusMsgParams<'_>,
) -> HandlerResult {
    let StatusMsgParams {
        channel_lower,
        original_target,
        msg,
        prefix_char,
        timestamp,
        msgid,
        snapshot,
    } = params;

    let opts = RouteOptions {
        send_away_reply: false,
        status_prefix: Some(prefix_char),
    };

    if route_to_channel_with_snapshot(
        ctx,
        channel_lower,
        msg,
        &opts,
        RouteMeta {
            timestamp,
            msgid,
            override_nick: None,
            relaymsg_sender_nick: None,
        },
        snapshot,
    )
    .await
        == ChannelRouteResult::NoSuchChannel
    {
        send_no_such_channel(ctx, &snapshot.nick, original_target).await?;
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

    #[test]
    fn test_parse_statusmsg_at() {
        let (prefix, target) = super::parse_statusmsg("@#channel");
        assert_eq!(prefix, Some('@'));
        assert_eq!(target, Some("#channel"));
    }

    #[test]
    fn test_parse_statusmsg_plus() {
        let (prefix, target) = super::parse_statusmsg("+#channel");
        assert_eq!(prefix, Some('+'));
        assert_eq!(target, Some("#channel"));
    }

    #[test]
    fn test_parse_statusmsg_percent() {
        let (prefix, target) = super::parse_statusmsg("%#channel");
        assert_eq!(prefix, Some('%'));
        assert_eq!(target, Some("#channel"));
    }

    #[test]
    fn test_parse_statusmsg_tilde() {
        let (prefix, target) = super::parse_statusmsg("~#channel");
        assert_eq!(prefix, Some('~'));
        assert_eq!(target, Some("#channel"));
    }

    #[test]
    fn test_parse_statusmsg_ampersand() {
        let (prefix, target) = super::parse_statusmsg("&#channel");
        assert_eq!(prefix, Some('&'));
        assert_eq!(target, Some("#channel"));
    }

    #[test]
    fn test_parse_statusmsg_regular_channel() {
        let (prefix, target) = super::parse_statusmsg("#channel");
        assert_eq!(prefix, None);
        assert_eq!(target, None);
    }

    #[test]
    fn test_parse_statusmsg_user() {
        let (prefix, target) = super::parse_statusmsg("nick");
        assert_eq!(prefix, None);
        assert_eq!(target, None);
    }

    #[test]
    fn test_parse_statusmsg_empty() {
        let (prefix, target) = super::parse_statusmsg("");
        assert_eq!(prefix, None);
        assert_eq!(target, None);
    }

    #[test]
    fn test_parse_statusmsg_prefix_without_channel() {
        let (prefix, target) = super::parse_statusmsg("@nick");
        assert_eq!(prefix, None);
        assert_eq!(target, None);
    }

    #[test]
    fn test_parse_statusmsg_double_prefix() {
        // @@#channel -> prefix '@', target '@#channel' (which is invalid channel name usually, but parse_statusmsg just splits)
        // Wait, let's check implementation:
        // if matches!(first_char, ...) && rest.chars().next().map(|c| c == '#' || c == '&' || c == '+' || c == '!').unwrap_or(false)
        // So for @@#channel: first='@', rest='@#channel'. rest[0] is '@', which is NOT in ['#', '&', '+', '!'].
        // So it should return (None, None).
        let (prefix, target) = super::parse_statusmsg("@@#channel");
        assert_eq!(prefix, None);
        assert_eq!(target, None);
    }
}
