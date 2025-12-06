//! NOTICE command handler.
//!
//! Per RFC 2812, NOTICE errors are silently ignored (no error replies).

use super::super::{
    Context, Handler, HandlerError, HandlerResult, user_mask_from_state, user_prefix,
};
use super::common::{ChannelRouteResult, RouteOptions, route_to_channel, route_to_user};
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
impl Handler for NoticeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // NOTICE <target> <text>
        let target = msg.arg(0).unwrap_or("");
        let text = msg.arg(1).unwrap_or("");

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
        }

        // Use shared validation (shun, rate limiting, spam detection)
        // NOTICE silently drops errors per RFC 2812
        validate_message_send(ctx, target, text, ErrorStrategy::SilentDrop).await?;

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Collect client-only tags (those starting with '+') to preserve them
        use slirc_proto::message::Tag;
        use std::borrow::Cow;
        let client_tags: Vec<Tag> = msg
            .tags_iter()
            .filter(|(k, _)| k.starts_with('+'))
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

        // Build the outgoing message with preserved client tags
        let out_msg = Message {
            tags: if client_tags.is_empty() {
                None
            } else {
                Some(client_tags)
            },
            prefix: Some(user_prefix(&nick, &user_name, &host)),
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
                // Route STATUSMSG
                let _ = super::privmsg::route_statusmsg(
                    ctx,
                    &channel_lower,
                    target,
                    out_msg,
                    prefix_char,
                )
                .await;
                debug!(from = %nick, to = %target, prefix = %prefix_char, "NOTICE STATUSMSG");
            } else if let ChannelRouteResult::Sent =
                route_to_channel(ctx, &channel_lower, out_msg, &opts).await
            {
                debug!(from = %nick, to = %target, "NOTICE to channel");
            }
            // All errors silently ignored for NOTICE
        } else {
            let target_lower = irc_to_lower(routing_target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, &nick).await {
                debug!(from = %nick, to = %target, "NOTICE to user");
            }
            // User not found: silently ignored for NOTICE
        }

        Ok(())
    }
}
