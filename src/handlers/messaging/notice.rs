//! NOTICE command handler.
//!
//! Per RFC 2812, NOTICE errors are silently ignored (no error replies).

use super::super::{Context,
    HandlerError, HandlerResult, PostRegHandler, user_prefix,
};
use crate::state::RegisteredState;
use super::common::{ChannelRouteResult, RouteOptions, SenderSnapshot, route_to_channel_with_snapshot, route_to_user_with_snapshot};
use super::validation::{ErrorStrategy, validate_message_send};
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, irc_to_lower};
use tracing::debug;

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

        // Build the outgoing message with preserved tags (client tags + label)
        let out_msg = Message {
            tags: if preserved_tags.is_empty() {
                None
            } else {
                Some(preserved_tags)
            },
            prefix: Some(user_prefix(&snapshot.nick, &snapshot.user, &snapshot.visible_host)),
            command: Command::NOTICE(target.to_string(), text.to_string()),
        };

        // NOTICE: silently drop on errors, check moderated, no away reply
        let opts = RouteOptions {
            send_away_reply: false,
            is_notice: true,
            block_ctcp: true,
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
                    &channel_lower,
                    target,
                    out_msg,
                    prefix_char,
                    None,
                    None,
                    &snapshot,
                )
                .await;
                debug!(from = %snapshot.nick, to = %target, prefix = %prefix_char, "NOTICE STATUSMSG");
                // Suppress ACK for echo-message with labels (echo IS the response)
                if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                    ctx.suppress_labeled_ack = true;
                }
            } else if let ChannelRouteResult::Sent =
                route_to_channel_with_snapshot(ctx, &channel_lower, out_msg, &opts, None, None, &snapshot).await
            {
                debug!(from = %snapshot.nick, to = %target, "NOTICE to channel");
                // Suppress ACK for echo-message with labels (echo IS the response)
                if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                    ctx.suppress_labeled_ack = true;
                }
            }
            // All errors silently ignored for NOTICE
        } else {
            let target_lower = irc_to_lower(routing_target);
            if route_to_user_with_snapshot(ctx, &target_lower, out_msg, &opts, None, None, &snapshot).await {
                debug!(from = %snapshot.nick, to = %target, "NOTICE to user");
            }
            // User not found: silently ignored for NOTICE
        }

        Ok(())
    }
}
