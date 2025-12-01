//! NOTICE command handler.
//!
//! Per RFC 2812, NOTICE errors are silently ignored (no error replies).

use super::common::{
    is_shunned, is_channel, route_to_channel, route_to_user, ChannelRouteResult, RouteOptions,
};
use super::super::{Context, Handler, HandlerError, HandlerResult, user_prefix};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, irc_to_lower};
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

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // Check message rate limit (NOTICE errors are silently ignored per RFC)
        let uid_string = ctx.uid.to_string();
        if !ctx.matrix.rate_limiter.check_message_rate(&uid_string) {
            return Ok(()); // Silently drop if rate limited
        }

        // NOTICE <target> <text>
        let target = msg.arg(0).unwrap_or("");
        let text = msg.arg(1).unwrap_or("");

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
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

        // Build the outgoing message
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::NOTICE(target.to_string(), text.to_string()),
        };

        // NOTICE: silently drop on errors, check moderated, no away reply
        let opts = RouteOptions {
            check_moderated: true,
            send_away_reply: false,
        };

        if is_channel(target) {
            let channel_lower = irc_to_lower(target);
            if let ChannelRouteResult::Sent =
                route_to_channel(ctx, &channel_lower, out_msg, &opts).await
            {
                debug!(from = %nick, to = %target, "NOTICE to channel");
            }
            // All errors silently ignored for NOTICE
        } else {
            let target_lower = irc_to_lower(target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
                debug!(from = %nick, to = %target, "NOTICE to user");
            }
            // User not found: silently ignored for NOTICE
        }

        Ok(())
    }
}
