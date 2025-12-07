//! Messaging command handlers (PRIVMSG, NOTICE, TAGMSG).
//!
//! Handles message routing to users and channels with support for:
//! - Channel modes (+n, +m, +r)
//! - Ban lists and quiet lists
//! - CTCP protocol
//! - Spam detection
//! - Service integration (NickServ, ChanServ)

mod common;
mod notice;
mod privmsg;
mod validation;

pub use notice::NoticeHandler;
pub use privmsg::PrivmsgHandler;

use super::{HandlerError, HandlerResult, user_mask_from_state, user_prefix};
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Tag, irc_to_lower};
use std::borrow::Cow;
use tracing::debug;

use common::{
    ChannelRouteResult, RouteOptions, is_shunned, route_to_channel, route_to_user,
    send_cannot_send, send_no_such_channel, send_no_such_nick,
};

// ============================================================================
// TAGMSG Handler
// ============================================================================

/// Handler for TAGMSG command.
///
/// IRCv3 message-tags: sends a message with only tags (no text body).
/// Requires the "message-tags" capability to be enabled.
pub struct TagmsgHandler;

#[async_trait]
impl crate::handlers::core::traits::PostRegHandler for TagmsgHandler {
    async fn handle(
        &self,
        ctx: &mut crate::handlers::core::traits::TypedContext<'_, crate::state::Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // Check if client has message-tags capability
        if !ctx.handshake.capabilities.contains("message-tags") {
            debug!("TAGMSG ignored: client lacks message-tags capability");
            return Ok(());
        }

        // TAGMSG <target>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let _nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let _user_name = ctx
            .handshake
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Collect only client-only tags (those starting with '+') AND the label tag
        // Server should not relay arbitrary tags from clients
        // The label tag is needed for labeled-response echoes
        // Unescape tag values since they come from wire format
        let tags: Option<Vec<Tag>> = if msg.tags.is_some() {
            let client_tags: Vec<Tag> = msg
                .tags_iter()
                .filter(|(k, _)| k.starts_with('+') || *k == "label")
                .map(|(k, v)| {
                    let value = if v.is_empty() {
                        None
                    } else {
                        // Unescape tag value from wire format
                        Some(slirc_proto::message::tags::unescape_tag_value(v))
                    };
                    Tag(Cow::Owned(k.to_string()), value)
                })
                .collect();
            if client_tags.is_empty() {
                None
            } else {
                Some(client_tags)
            }
        } else {
            None
        };

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Build the outgoing TAGMSG
        let out_msg = Message {
            tags,
            prefix: Some(user_prefix(&nick, &user_name, &host)),
            command: Command::TAGMSG(target.to_string()),
        };

        // TAGMSG: send errors, but don't check +m (only +n), no away reply
        let opts = RouteOptions {
            send_away_reply: false,
            is_notice: false,
            block_ctcp: false,
            status_prefix: None,
        };

        if target.is_channel_name() {
            let channel_lower = irc_to_lower(target);
            match route_to_channel(ctx, &channel_lower, out_msg, &opts, None, None).await {
                ChannelRouteResult::Sent => {
                    debug!(from = %nick, to = %target, "TAGMSG to channel");
                    // Suppress ACK for echo-message with labels (echo IS the response)
                    if ctx.label.is_some() && ctx.handshake.capabilities.contains("echo-message") {
                        ctx.suppress_labeled_ack = true;
                    }
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, &nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, &nick, target, "Cannot send to channel (+n)").await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    // TAGMSG doesn't check +m, so this shouldn't happen
                    unreachable!("TAGMSG should not check moderated mode");
                }
                ChannelRouteResult::BlockedRegisteredOnly => {
                    send_cannot_send(ctx, &nick, target, "Cannot send to channel (+r)").await?;
                }
                ChannelRouteResult::BlockedCTCP => {
                    // TAGMSG has no CTCP, so this shouldn't happen
                    unreachable!("TAGMSG has no CTCP content");
                }
                ChannelRouteResult::BlockedNotice => {
                    // TAGMSG is not a NOTICE, so this shouldn't happen
                    unreachable!("TAGMSG is not a NOTICE");
                }
                ChannelRouteResult::BlockedBanned => {
                    send_cannot_send(ctx, &nick, target, "Cannot send to channel (+b)").await?;
                }
            }
        } else {
            let target_lower = irc_to_lower(target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, &nick, None, None).await {
                debug!(from = %nick, to = %target, "TAGMSG to user");
            } else {
                send_no_such_nick(ctx, &nick, target).await?;
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::handlers::matches_hostmask;
    use slirc_proto::ChannelExt;

    #[test]
    fn test_is_channel() {
        assert!("#rust".is_channel_name());
        assert!("&local".is_channel_name());
        assert!("+modeless".is_channel_name());
        assert!("!safe".is_channel_name());
        assert!(!"nickname".is_channel_name());
        assert!(!"NickServ".is_channel_name());
    }

    #[test]
    fn test_matches_hostmask_exact() {
        assert!(matches_hostmask("nick!user@host", "nick!user@host"));
        assert!(!matches_hostmask("nick!user@host", "other!user@host"));
    }

    #[test]
    fn test_matches_hostmask_wildcard_star() {
        assert!(matches_hostmask("*!*@*", "nick!user@host"));
        assert!(matches_hostmask("nick!*@*", "nick!user@host"));
        assert!(matches_hostmask("*!user@*", "nick!user@host"));
        assert!(matches_hostmask("*!*@host", "nick!user@host"));
        assert!(matches_hostmask(
            "*!*@*.example.com",
            "nick!user@sub.example.com"
        ));
    }

    #[test]
    fn test_matches_hostmask_wildcard_question() {
        assert!(matches_hostmask("nic?!user@host", "nick!user@host"));
        assert!(matches_hostmask("????!user@host", "nick!user@host"));
        assert!(!matches_hostmask("???!user@host", "nick!user@host"));
    }

    #[test]
    fn test_matches_hostmask_case_insensitive() {
        assert!(matches_hostmask("NICK!USER@HOST", "nick!user@host"));
        assert!(matches_hostmask("Nick!User@Host", "NICK!USER@HOST"));
    }
}
