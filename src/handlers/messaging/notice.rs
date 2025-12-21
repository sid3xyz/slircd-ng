//! NOTICE command handler.
//!
//! Per RFC 2812, NOTICE errors are silently ignored (no error replies).

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, user_prefix};
use super::common::{
    ChannelRouteResult, RouteOptions, SenderSnapshot, UserRouteResult,
    route_to_channel_with_snapshot, route_to_user_with_snapshot,
};
use super::validation::{ErrorStrategy, validate_message_send};
use crate::history::types::MessageTag as HistoryTag;
use crate::history::{MessageEnvelope, StoredMessage};
use crate::state::RegisteredState;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use slirc_proto::{ChannelExt, Command, Message, MessageRef, irc_to_lower};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;
use uuid::Uuid;

// ============================================================================
// NOTICE Handler
// ============================================================================

/// Handler for NOTICE command.
///
/// Per RFC 2812, NOTICE errors are silently ignored (no error replies).
pub struct NoticeHandler;

#[async_trait]
impl PostRegHandler for NoticeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // NOTICE <target> <text>
        let target = msg.arg(0).unwrap_or("");
        let text = msg.arg(1).unwrap_or("");

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
        }

        // Build sender snapshot once (eliminates redundant user reads across validation + routing)
        let snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Use shared validation (shun, rate limiting, spam detection)
        // NOTICE silently drops errors per RFC 2812
        validate_message_send(ctx, target, text, ErrorStrategy::SilentDrop, &snapshot).await?;

        // Collect client-only tags (those starting with '+') AND the label tag to preserve them
        // The label tag is needed for labeled-response echoes back to the sender
        use slirc_proto::message::Tag;
        use std::borrow::Cow;
        let preserved_tags: Vec<Tag> = msg
            .tags_iter()
            .filter(|(k, _)| k.starts_with('+') || *k == "label")
            .map(|(k, v)| {
                Tag(
                    Cow::Owned(k.to_string()),
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    },
                )
            })
            .collect();

        // Generate timestamp and msgid
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let millis = duration.as_millis() as i64;
        let nanotime = millis * 1_000_000;

        let dt = DateTime::<Utc>::from_timestamp(millis / 1000, (millis % 1000) as u32 * 1_000_000)
            .unwrap_or_default();
        let timestamp_iso = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let msgid = Uuid::new_v4().to_string();

        // Prepare tags for history
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

        // Build the outgoing message with preserved tags (client tags + label)
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
            command: Command::NOTICE(target.to_string(), text.to_string()),
        };

        // NOTICE: silently drop on errors, check moderated, no away reply
        let opts = RouteOptions {
            send_away_reply: false,

            status_prefix: None,
        };

        // STATUSMSG support: @#channel sends to ops, +#channel sends to voiced+
        let (status_prefix, actual_target) = super::privmsg::parse_statusmsg(target);
        let routing_target = actual_target.unwrap_or(target);

        if routing_target.is_channel_name() {
            let channel_lower = irc_to_lower(routing_target);

            if let Some(prefix_char) = status_prefix {
                // Route STATUSMSG with snapshot
                let _ = super::privmsg::route_statusmsg(
                    ctx,
                    super::privmsg::StatusMsgParams {
                        channel_lower: &channel_lower,
                        original_target: target,
                        msg: out_msg,
                        prefix_char,
                        timestamp: Some(timestamp_iso.clone()),
                        msgid: Some(msgid.clone()),
                        snapshot: &snapshot,
                    },
                )
                .await;
                debug!(from = %snapshot.nick, to = %target, prefix = %prefix_char, "NOTICE STATUSMSG");
                // Suppress ACK for echo-message with labels (echo IS the response)
                if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                    ctx.suppress_labeled_ack = true;
                }
            } else if let ChannelRouteResult::Sent = route_to_channel_with_snapshot(
                ctx,
                &channel_lower,
                out_msg,
                &opts,
                Some(timestamp_iso.clone()),
                Some(msgid.clone()),
                &snapshot,
            )
            .await
            {
                debug!(from = %snapshot.nick, to = %target, "NOTICE to channel");
                // Suppress ACK for echo-message with labels (echo IS the response)
                if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                    ctx.suppress_labeled_ack = true;
                }

                // Store message in history
                let prefix = format!(
                    "{}!{}@{}",
                    snapshot.nick, snapshot.user, snapshot.visible_host
                );
                let envelope = MessageEnvelope {
                    command: "NOTICE".to_string(),
                    prefix: prefix.clone(),
                    target: target.to_string(),
                    text: text.to_string(),
                    tags: history_tags.clone(),
                };
                let stored_msg = StoredMessage {
                    msgid: msgid.clone(),
                    target: irc_to_lower(target),
                    sender: snapshot.nick.clone(),
                    envelope,
                    nanotime,
                    account: ctx.state.account.clone(),
                };
                if let Err(e) = ctx
                    .matrix
                    .service_manager
                    .history
                    .store(target, stored_msg)
                    .await
                {
                    debug!(error = %e, "Failed to store NOTICE in history");
                }
            }
            // All errors silently ignored for NOTICE
        } else {
            let target_lower = irc_to_lower(routing_target);
            if route_to_user_with_snapshot(
                ctx,
                &target_lower,
                out_msg,
                &opts,
                Some(timestamp_iso.clone()),
                Some(msgid.clone()),
                &snapshot,
            )
            .await
                == UserRouteResult::Sent
            {
                debug!(from = %snapshot.nick, to = %target, "NOTICE to user");

                // Store message in history (DMs)
                let prefix = format!(
                    "{}!{}@{}",
                    snapshot.nick, snapshot.user, snapshot.visible_host
                );
                let envelope = MessageEnvelope {
                    command: "NOTICE".to_string(),
                    prefix: prefix.clone(),
                    target: target.to_string(),
                    text: text.to_string(),
                    tags: history_tags.clone(),
                };
                let stored_msg = StoredMessage {
                    msgid: msgid.clone(),
                    target: irc_to_lower(target),
                    sender: snapshot.nick.clone(),
                    envelope,
                    nanotime,
                    account: ctx.state.account.clone(),
                };

                // Store for recipient
                if let Err(e) = ctx
                    .matrix
                    .service_manager
                    .history
                    .store(target, stored_msg.clone())
                    .await
                {
                    debug!(error = %e, "Failed to store NOTICE DM for recipient");
                }
                // Store for sender
                if let Err(e) = ctx
                    .matrix
                    .service_manager
                    .history
                    .store(&snapshot.nick, stored_msg)
                    .await
                {
                    debug!(error = %e, "Failed to store NOTICE DM for sender");
                }
            }
            // User not found: silently ignored for NOTICE
        }

        Ok(())
    }
}
