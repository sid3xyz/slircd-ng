//! Message delivery and construction helpers.
//!
//! Handles building messages for local delivery, including capability filtering
//! (echo-message, message-tags, server-time) and error responses.

use super::super::{HandlerResult, server_reply};
use super::types::SenderSnapshot;
use crate::handlers::core::Context;
use slirc_proto::{Message, Response};
use std::collections::HashSet;

/// Build a recipient-specific message for a local user, applying capability-aware tag filtering.
pub fn build_local_recipient_message(
    base: &Message,
    recipient_caps: &HashSet<String>,
    snapshot: &SenderSnapshot,
    msgid_str: &str,
    timestamp_str: &str,
    sender_label: Option<&String>,
) -> Message {
    let has_message_tags = recipient_caps.contains("message-tags");
    let has_server_time = recipient_caps.contains("server-time");

    let mut result = base.clone();

    // Strip label tag from recipient copies (label is sender-only)
    if sender_label.is_some() {
        result.tags = result
            .tags
            .map(|tags| {
                tags.into_iter()
                    .filter(|tag| tag.0.as_ref() != "label")
                    .collect::<Vec<_>>()
            })
            .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
    }

    // If recipient doesn't have message-tags, strip client-only tags
    if !has_message_tags {
        result.tags = result
            .tags
            .map(|tags| {
                tags.into_iter()
                    .filter(|tag| !tag.0.starts_with('+'))
                    .collect::<Vec<_>>()
            })
            .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
    } else {
        // Add msgid for users with message-tags
        result = result.with_tag("msgid", Some(msgid_str.to_string()));
    }

    // Add server-time if capability is enabled
    if has_server_time {
        result = result.with_tag("time", Some(timestamp_str.to_string()));
    }

    // Add account-tag if sender is logged in and recipient has capability
    if let Some(account) = snapshot.account.as_ref()
        && recipient_caps.contains("account-tag")
    {
        result = result.with_tag("account", Some(account.clone()));
    }

    // Add bot tag if sender is a bot and recipient has message-tags
    if snapshot.is_bot && has_message_tags {
        result = result.with_tag("bot", None::<String>);
    }

    result
}

// ============================================================================
// Error Response Helpers
// ============================================================================

/// Send ERR_CANNOTSENDTOCHAN with the given reason.
pub async fn send_cannot_send<S>(
    ctx: &Context<'_, S>,
    nick: &str,
    target: &str,
    reason: &str,
) -> HandlerResult {
    let reply = server_reply(
        ctx.server_name(),
        Response::ERR_CANNOTSENDTOCHAN,
        vec![nick.to_string(), target.to_string(), reason.to_string()],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHCHANNEL.
pub async fn send_no_such_channel<S>(
    ctx: &Context<'_, S>,
    nick: &str,
    target: &str,
) -> HandlerResult {
    let reply = Response::err_nosuchchannel(nick, target).with_prefix(ctx.server_prefix());
    ctx.send_error("PRIVMSG", "ERR_NOSUCHCHANNEL", reply)
        .await?;
    Ok(())
}
