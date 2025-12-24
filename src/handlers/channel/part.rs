//! PART command handler.
//!
//! # RFC 2812 §3.2.2 - Part message
//!
//! Removes a user from a channel.
//!
//! **Specification:** [RFC 2812 §3.2.2](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.2)
//!
//! **Compliance:** 5/5 irctest pass
//!
//! ## Syntax
//! ```text
//! PART <channels> [<reason>]
//! ```
//!
//! ## Behavior
//! - Can part multiple channels (comma-separated)
//! - Optional part message broadcast to channel
//! - User must be in channel to part it
//! - Destroys empty transient channels
//! - Persists state for registered channels

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, server_reply, user_mask_from_state,
};
use super::common::{parse_channel_list, parse_reason};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelError, ChannelEvent};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

pub struct PartHandler;

#[async_trait]
impl PostRegHandler for PartHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // PART <channels> [reason]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let reason = parse_reason(msg.arg(1));

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        for channel_name in parse_channel_list(channels_str) {
            let channel_lower = irc_to_lower(channel_name);
            leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, reason).await?;
        }

        Ok(())
    }
}

/// Internal function to leave a channel.
pub(super) async fn leave_channel_internal<S>(
    ctx: &mut Context<'_, S>,
    channel_lower: &str,
    nick: &str,
    user_name: &str,
    host: &str,
    reason: Option<&str>,
) -> HandlerResult {
    // Check if channel exists
    let channel_sender = match ctx.matrix.channel_manager.channels.get(channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply =
                Response::err_nosuchchannel(nick, channel_lower).with_prefix(ctx.server_prefix());
            ctx.send_error("PART", "ERR_NOSUCHCHANNEL", reply).await?;
            return Ok(());
        }
    };

    let prefix = Prefix::new(nick.to_string(), user_name.to_string(), host.to_string());

    let (reply_tx, reply_rx) = oneshot::channel();
    let event = ChannelEvent::Part {
        uid: ctx.uid.to_string(),
        reason: reason.map(|s| s.to_string()),
        prefix,
        reply_tx,
    };

    if (channel_sender.send(event).await).is_err() {
        // Channel actor died, remove it
        ctx.matrix.channel_manager.channels.remove(channel_lower);
        return Ok(());
    }

    match reply_rx.await {
        Ok(Ok(remaining_members)) => {
            // Success
            // Remove channel from user's list
            if let Some(user) = ctx.matrix.user_manager.users.get(ctx.uid) {
                let mut user = user.write().await;
                user.channels.remove(channel_lower);
            }

            if remaining_members == 0 {
                ctx.matrix.channel_manager.channels.remove(channel_lower);
                crate::metrics::ACTIVE_CHANNELS.dec();
            }

            info!(nick = %nick, channel = %channel_lower, "User left channel");
        }
        Ok(Err(e)) => {
            let reply = match e {
                ChannelError::NotOnChannel => {
                    Response::err_notonchannel(ctx.server_name(), channel_lower)
                }
                _ => server_reply(
                    ctx.server_name(),
                    Response::ERR_NOTONCHANNEL,
                    vec![nick.to_string(), channel_lower.to_string(), e.to_string()],
                ),
            };
            ctx.sender.send(reply).await?;
        }
        Err(_) => {
            // Actor dropped
            ctx.matrix.channel_manager.channels.remove(channel_lower);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::common::{parse_channel_list, parse_reason};
    use slirc_proto::irc_to_lower;

    // ========================================================================
    // PART-specific integration tests
    // Tests the combination of parsing + lowercase conversion used by PART
    // ========================================================================

    #[test]
    fn test_part_single_channel() {
        let channels = parse_channel_list("#Test");
        assert_eq!(channels.len(), 1);
        assert_eq!(irc_to_lower(channels[0]), "#test");
    }

    #[test]
    fn test_part_multiple_channels() {
        let channels = parse_channel_list("#Foo,#Bar,#Baz");
        assert_eq!(channels.len(), 3);

        let lowered: Vec<String> = channels.iter().map(|c| irc_to_lower(c)).collect();
        assert_eq!(lowered, vec!["#foo", "#bar", "#baz"]);
    }

    #[test]
    fn test_part_with_reason() {
        let reason = parse_reason(Some("Goodbye everyone!"));
        assert_eq!(reason, Some("Goodbye everyone!"));
    }

    #[test]
    fn test_part_without_reason() {
        let reason = parse_reason(None);
        assert_eq!(reason, None);
    }

    #[test]
    fn test_part_empty_reason() {
        let reason = parse_reason(Some(""));
        assert_eq!(reason, None);
    }

    #[test]
    fn test_part_whitespace_reason() {
        let reason = parse_reason(Some("   "));
        assert_eq!(reason, None);
    }

    #[test]
    fn test_part_reason_with_leading_trailing_whitespace() {
        let reason = parse_reason(Some("  Leaving now  "));
        assert_eq!(reason, Some("Leaving now"));
    }

    // ========================================================================
    // Edge cases specific to PART
    // ========================================================================

    #[test]
    fn test_part_channel_list_with_whitespace() {
        let channels = parse_channel_list(" #foo , #bar ");
        assert_eq!(channels, vec!["#foo", "#bar"]);
    }

    #[test]
    fn test_part_channel_list_empty_entries() {
        let channels = parse_channel_list("#foo,,#bar");
        assert_eq!(channels, vec!["#foo", "#bar"]);
    }

    #[test]
    fn test_part_channel_list_only_commas() {
        let channels = parse_channel_list(",,,");
        assert!(channels.is_empty());
    }

    #[test]
    fn test_part_channel_case_preservation() {
        // PART should preserve display case but compare lowercase
        let channels = parse_channel_list("#MyChannel");
        assert_eq!(channels[0], "#MyChannel"); // Preserved
        assert_eq!(irc_to_lower(channels[0]), "#mychannel"); // Lowered for lookup
    }

    #[test]
    fn test_part_reason_unicode() {
        let reason = parse_reason(Some("Goodbye! 👋"));
        assert_eq!(reason, Some("Goodbye! 👋"));
    }

    #[test]
    fn test_part_reason_multiword() {
        let reason = parse_reason(Some("See you all later, friends!"));
        assert_eq!(reason, Some("See you all later, friends!"));
    }
}
