//! Messaging command handlers (PRIVMSG, NOTICE, TAGMSG).
//!
//! Handles message routing to users and channels with support for:
//! - Channel modes (+n, +m, +r)
//! - Ban lists and quiet lists
//! - CTCP protocol
//! - Spam detection
//! - Service integration (NickServ, ChanServ)
//! - Event-sourced history (Innovation 5)

mod accept;
mod common;
mod errors;
mod metadata;
mod notice;
mod npc;
mod privmsg;
mod relaymsg;
mod validation;

pub use accept::AcceptHandler;
pub use metadata::MetadataHandler;
pub use notice::NoticeHandler;
pub use npc::NpcHandler;
pub use privmsg::PrivmsgHandler;
pub use relaymsg::RelayMsgHandler;

use super::{HandlerError, HandlerResult, user_prefix};
use crate::history::types::MessageTag as HistoryTag;
use crate::history::{MessageEnvelope, StoredMessage};
use crate::state::dashmap_ext::DashMapExt;
use async_trait::async_trait;
use errors::*;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Tag, irc_to_lower};
use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;
use uuid::Uuid;

use common::{
    ChannelRouteResult, RouteOptions, SenderSnapshot, UserRouteResult, is_shunned_with_snapshot,
    route_to_channel_with_snapshot, route_to_user_with_snapshot, send_cannot_send,
    send_no_such_channel,
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
        ctx: &mut crate::handlers::core::context::Context<'_, crate::state::RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // Check shun first - silently ignore if shunned
        // Build snapshot first for shun check
        let snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        if is_shunned_with_snapshot(ctx, &snapshot).await {
            return Ok(());
        }

        // Check if client has message-tags capability
        if !ctx.state.capabilities.contains("message-tags") {
            debug!("TAGMSG ignored: client lacks message-tags capability");
            return Ok(());
        }

        // TAGMSG <target>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Check for +draft/persist tag (required for history storage per IRCv3)
        let has_persist_tag = msg.tags_iter().any(|(k, _)| k == "+draft/persist");

        // Generate msgid for history storage and echo-message (with dashes for IRCv3 compatibility)
        let msgid = Uuid::new_v4().to_string();

        // Collect only client-only tags (those starting with '+') AND the label tag
        // Server should not relay arbitrary tags from clients
        // The label tag is needed for labeled-response echoes
        // Unescape tag values since they come from wire format
        // Also track which tags to persist for history
        let mut persist_tags: Vec<(String, Option<String>)> = Vec::with_capacity(4);
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
                    // Track client-only tags for persistence (not label)
                    if k.starts_with('+') {
                        persist_tags.push((k.to_string(), value.clone()));
                    }
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

        // Add msgid tag for outgoing message
        let mut out_tags = tags.unwrap_or_default();
        out_tags.push(Tag(Cow::Borrowed("msgid"), Some(msgid.clone())));
        let tags = Some(out_tags);

        // Build the outgoing TAGMSG using snapshot
        let out_msg = Message {
            tags,
            prefix: Some(user_prefix(
                &snapshot.nick,
                &snapshot.user,
                &snapshot.visible_host,
            )),
            command: Command::TAGMSG(target.to_string()),
        };

        // TAGMSG: send errors, but don't check +m (only +n), no away reply
        let opts = RouteOptions {
            send_away_reply: false,
            status_prefix: None,
        };

        if target.is_channel_name() {
            let channel_lower = irc_to_lower(target);
            // Pass msgid to route function to ensure consistency between echo and history
            match route_to_channel_with_snapshot(
                ctx,
                &channel_lower,
                out_msg,
                &opts,
                None,
                Some(msgid.clone()),
                &snapshot,
            )
            .await
            {
                ChannelRouteResult::Sent => {
                    debug!(from = %snapshot.nick, to = %target, "TAGMSG to channel");
                    // Suppress ACK for echo-message with labels (echo IS the response)
                    if ctx.label.is_some() && ctx.state.capabilities.contains("echo-message") {
                        ctx.suppress_labeled_ack = true;
                    }

                    // Store TAGMSG in history if +draft/persist tag is present (Innovation 5)
                    if has_persist_tag && ctx.matrix.config.history.should_store_event("TAGMSG") {
                        let prefix = format!(
                            "{}!{}@{}",
                            snapshot.nick, snapshot.user, snapshot.visible_host
                        );
                        let nanotime = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as i64;

                        let history_tags: Option<Vec<HistoryTag>> = if !persist_tags.is_empty() {
                            Some(
                                persist_tags
                                    .iter()
                                    .map(|t| HistoryTag {
                                        key: t.0.to_string(),
                                        value: t.1.clone(),
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        };

                        let envelope = MessageEnvelope {
                            command: "TAGMSG".to_string(),
                            prefix: prefix.clone(),
                            target: target.to_string(),
                            text: String::new(), // TAGMSG has no text
                            tags: history_tags,
                        };

                        let stored_msg = StoredMessage {
                            msgid: msgid.clone(),
                            target: channel_lower.clone(),
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
                            debug!(error = %e, "Failed to store TAGMSG in history");
                        }
                    }
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, &snapshot.nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_NOT_IN_CHANNEL)
                        .await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    // TAGMSG doesn't check +m, so this shouldn't happen
                    unreachable!("TAGMSG should not check moderated mode");
                }
                ChannelRouteResult::BlockedRegisteredOnly => {
                    send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_REGISTERED_ONLY)
                        .await?;
                }
                ChannelRouteResult::BlockedRegisteredSpeak => {
                    send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_REGISTERED_SPEAK)
                        .await?;
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
                    send_cannot_send(ctx, &snapshot.nick, target, CANNOT_SEND_BANNED).await?;
                }
            }
        } else {
            let target_lower = irc_to_lower(target);
            // Pass msgid to route function to ensure consistency between echo and history
            if route_to_user_with_snapshot(
                ctx,
                &target_lower,
                out_msg,
                &opts,
                None,
                Some(msgid.clone()),
                &snapshot,
            )
            .await
                == UserRouteResult::Sent
            {
                debug!(from = %snapshot.nick, to = %target, "TAGMSG to user");

                // Store TAGMSG in history for DMs if +draft/persist tag is present (Innovation 5)
                if has_persist_tag && ctx.matrix.config.history.should_store_event("TAGMSG") {
                    let prefix = format!(
                        "{}!{}@{}",
                        snapshot.nick, snapshot.user, snapshot.visible_host
                    );
                    let nanotime = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as i64;

                    let history_tags: Option<Vec<HistoryTag>> = if !persist_tags.is_empty() {
                        Some(
                            persist_tags
                                .iter()
                                .map(|t| HistoryTag {
                                    key: t.0.to_string(),
                                    value: t.1.clone(),
                                })
                                .collect(),
                        )
                    } else {
                        None
                    };

                    let envelope = MessageEnvelope {
                        command: "TAGMSG".to_string(),
                        prefix: prefix.clone(),
                        target: target.to_string(),
                        text: String::new(),
                        tags: history_tags,
                    };

                    let stored_msg = StoredMessage {
                        msgid: msgid.clone(),
                        target: target_lower.clone(),
                        sender: snapshot.nick.clone(),
                        envelope,
                        nanotime,
                        account: ctx.state.account.clone(),
                    };

                    // Build DM key (same pattern as PRIVMSG)
                    let sender_key_part = if let Some(acct) = &snapshot.account {
                        format!("a:{}", irc_to_lower(acct))
                    } else {
                        format!("u:{}", irc_to_lower(&snapshot.nick))
                    };

                    let target_account = if let Some(uid) =
                        ctx.matrix.user_manager.nicks.get_cloned(&target_lower)
                    {
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
                    let dm_key = format!("dm:{}:{}", users[0], users[1]);

                    if let Err(e) = ctx
                        .matrix
                        .service_manager
                        .history
                        .store(&dm_key, stored_msg)
                        .await
                    {
                        debug!(error = %e, "Failed to store TAGMSG DM in history");
                    }
                }
            } else {
                crate::handlers::send_no_such_nick(ctx, "TAGMSG", target).await?;
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
