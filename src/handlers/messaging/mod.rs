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

pub use notice::NoticeHandler;
pub use privmsg::PrivmsgHandler;

use super::{Context, Handler, HandlerError, HandlerResult, user_prefix};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Tag, irc_to_lower};
use std::borrow::Cow;
use tracing::debug;

use common::{
    is_shunned, is_channel, route_to_channel, route_to_user, send_cannot_send,
    send_no_such_channel, send_no_such_nick, ChannelRouteResult, RouteOptions,
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
impl Handler for TagmsgHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

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

        // Convert tags from MessageRef to owned Tag structs
        let tags: Option<Vec<Tag>> = if msg.tags.is_some() {
            Some(
                msg.tags_iter()
                    .map(|(k, v)| {
                        let value = if v.is_empty() {
                            None
                        } else {
                            Some(v.to_string())
                        };
                        Tag(Cow::Owned(k.to_string()), value)
                    })
                    .collect(),
            )
        } else {
            None
        };

        // Build the outgoing TAGMSG
        let out_msg = Message {
            tags,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::TAGMSG(target.to_string()),
        };

        // TAGMSG: send errors, but don't check +m (only +n), no away reply
        let opts = RouteOptions {
            check_moderated: false,
            send_away_reply: false,
            is_notice: false,
            strip_colors: false,
            block_ctcp: false,
        };

        if is_channel(target) {
            let channel_lower = irc_to_lower(target);
            match route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
                ChannelRouteResult::Sent => {
                    debug!(from = %nick, to = %target, "TAGMSG to channel");
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+n)").await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    // TAGMSG doesn't check +m, so this shouldn't happen
                    unreachable!("TAGMSG should not check moderated mode");
                }
                ChannelRouteResult::BlockedSpam => {
                    // TAGMSG has no text, so spam detection shouldn't trigger
                    unreachable!("TAGMSG has no text content to check for spam");
                }
                ChannelRouteResult::BlockedRegisteredOnly => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+r)").await?;
                }
                ChannelRouteResult::BlockedCTCP => {
                    // TAGMSG has no CTCP, so this shouldn't happen
                    unreachable!("TAGMSG has no CTCP content");
                }
                ChannelRouteResult::BlockedNotice => {
                    // TAGMSG is not a NOTICE, so this shouldn't happen
                    unreachable!("TAGMSG is not a NOTICE");
                }
            }
        } else {
            let target_lower = irc_to_lower(target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
                debug!(from = %nick, to = %target, "TAGMSG to user");
            } else {
                send_no_such_nick(ctx, nick, target).await?;
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
    use super::common::is_channel;
    use crate::handlers::matches_hostmask;

    #[test]
    fn test_is_channel() {
        assert!(is_channel("#rust"));
        assert!(is_channel("&local"));
        assert!(is_channel("+modeless"));
        assert!(is_channel("!safe"));
        assert!(!is_channel("nickname"));
        assert!(!is_channel("NickServ"));
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
        assert!(matches_hostmask("*!*@*.example.com", "nick!user@sub.example.com"));
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
