//! Channel membership operations.
//!
//! Used by both regular JOIN/PART and admin SA* commands.
//! These functions perform the core channel membership operations without
//! permission checks, allowing callers to implement their own access control.

use super::super::{server_reply, user_prefix, with_label, Context, HandlerResult};
use crate::state::{Channel, MemberModes};
use slirc_proto::{Command, Message, Prefix, Response, irc_to_lower};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Target user information for channel operations.
///
/// Bundles user identity info needed for JOIN/PART messages.
pub struct TargetUser<'a> {
    /// User's unique ID
    pub uid: &'a str,
    /// User's nick
    pub nick: &'a str,
    /// User's username
    pub user: &'a str,
    /// User's host
    pub host: &'a str,
}

/// Add a user to a channel without any permission checks.
///
/// This is the core operation used by JOIN (after checks) and SAJOIN.
///
/// - Creates channel if it doesn't exist
/// - Adds user as member with specified modes
/// - Updates user's channel list
/// - Broadcasts JOIN to channel
/// - Sends topic and names to joining user
///
/// # Arguments
/// * `ctx` - Handler context
/// * `target` - Target user info (uid, nick, user, host)
/// * `channel_name` - Channel to join (will be created if needed)
/// * `modes` - Initial member modes (e.g., op for first user)
/// * `send_topic_names_to` - If Some, send topic/names to this sender (the joining user)
///
/// Caller is responsible for:
/// - Validating channel name
/// - Checking permissions (invite-only, bans, etc.) if applicable
#[allow(clippy::too_many_arguments)]
pub async fn force_join_channel(
    ctx: &Context<'_>,
    target: &TargetUser<'_>,
    channel_name: &str,
    modes: MemberModes,
    send_topic_names_to: Option<&tokio::sync::mpsc::Sender<Message>>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);

    // Get or create channel
    let channel_ref = ctx
        .matrix
        .channels
        .entry(channel_lower.clone())
        .or_insert_with(|| {
            crate::metrics::ACTIVE_CHANNELS.inc();
            Arc::new(RwLock::new(Channel::new(channel_name.to_string())))
        })
        .clone();

    // Add user to channel
    let (topic, canonical_name, member_count) = {
        let mut channel = channel_ref.write().await;
        if !channel.is_member(target.uid) {
            channel.add_member(target.uid.to_string(), modes.clone());
        }
        (
            channel.topic.clone(),
            channel.name.clone(),
            channel.members.len(),
        )
    };

    // Add channel to user's list
    if let Some(user_ref) = ctx.matrix.users.get(target.uid) {
        let mut user = user_ref.write().await;
        user.channels.insert(channel_lower.clone());
    }

    // Build and broadcast JOIN message
    let join_msg = Message {
        tags: None,
        prefix: Some(Prefix::Nickname(
            target.nick.to_string(),
            target.user.to_string(),
            target.host.to_string(),
        )),
        command: Command::JOIN(canonical_name.clone(), None, None),
    };
    ctx.matrix
        .broadcast_to_channel(&channel_lower, join_msg, None)
        .await;

    info!(
        nick = %target.nick,
        channel = %canonical_name,
        members = member_count,
        "User joined channel"
    );

    // Send topic and names to joining user if requested
    if let Some(sender) = send_topic_names_to {
        // Send topic if set
        if let Some(topic) = topic {
            let topic_reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_TOPIC,
                vec![
                    target.nick.to_string(),
                    canonical_name.clone(),
                    topic.text,
                ],
            );
            sender.send(topic_reply).await?;
        }

        // Send NAMES list
        let channel = ctx.matrix.channels.get(&channel_lower).unwrap();
        let channel = channel.read().await;
        let mut names_list = Vec::new();
        for (uid, member_modes) in &channel.members {
            if let Some(user) = ctx.matrix.users.get(uid) {
                let user = user.read().await;
                let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                    format!("{}{}", prefix, user.nick)
                } else {
                    user.nick.clone()
                };
                names_list.push(nick_with_prefix);
            }
        }
        // Channel symbol per RFC 2812: @ = secret, = = public
        let channel_symbol = if channel.modes.secret { "@" } else { "=" };
        drop(channel);

        let names_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_NAMREPLY,
            vec![
                target.nick.to_string(),
                channel_symbol.to_string(),
                canonical_name.clone(),
                names_list.join(" "),
            ],
        );
        sender.send(names_reply).await?;

        let end_names = with_label(
            server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    target.nick.to_string(),
                    canonical_name,
                    "End of /NAMES list".to_string(),
                ],
            ),
            ctx.label.as_deref(),
        );
        sender.send(end_names).await?;
    }

    Ok(())
}

/// Remove a user from a channel without any permission checks.
///
/// This is the core operation used by PART (after checks) and SAPART.
///
/// - Broadcasts PART to channel
/// - Removes user from channel
/// - Updates user's channel list
/// - Removes channel if empty and not permanent
///
/// # Arguments
/// * `ctx` - Handler context
/// * `target` - Target user info (uid, nick, user, host)
/// * `channel_lower` - Lowercased channel name
/// * `reason` - Optional part reason
///
/// Returns `Ok(true)` if user was in channel and removed,
/// `Ok(false)` if user was not in channel (caller may want to send error).
pub async fn force_part_channel(
    ctx: &Context<'_>,
    target: &TargetUser<'_>,
    channel_lower: &str,
    reason: Option<&str>,
) -> Result<bool, super::super::HandlerError> {
    // Get channel reference
    let Some(channel_ref) = ctx.matrix.channels.get(channel_lower).map(|r| r.clone()) else {
        return Ok(false);
    };

    let mut channel_guard = channel_ref.write().await;

    // Check if user is in channel
    if !channel_guard.is_member(target.uid) {
        return Ok(false);
    }

    let canonical_name = channel_guard.name.clone();

    // Broadcast PART before removing
    let part_msg = Message {
        tags: None,
        prefix: Some(user_prefix(target.nick, target.user, target.host)),
        command: Command::PART(canonical_name.clone(), reason.map(String::from)),
    };

    // Broadcast to all members including target
    for uid in channel_guard.members.keys() {
        if let Some(sender) = ctx.matrix.senders.get(uid) {
            let _ = sender.send(part_msg.clone()).await;
        }
    }

    // Remove user from channel
    channel_guard.remove_member(target.uid);
    let is_empty = channel_guard.members.is_empty();
    let is_permanent = channel_guard.modes.permanent;

    drop(channel_guard);

    // Remove channel from user's list
    if let Some(user) = ctx.matrix.users.get(target.uid) {
        let mut user = user.write().await;
        user.channels.remove(channel_lower);
    }

    // If channel is now empty and not permanent (+P), remove it
    if is_empty && !is_permanent {
        ctx.matrix.channels.remove(channel_lower);
        crate::metrics::ACTIVE_CHANNELS.dec();
        debug!(channel = %canonical_name, "Channel removed (empty)");
    } else if is_empty && is_permanent {
        debug!(channel = %canonical_name, "Channel kept alive (+P permanent)");
    }

    info!(nick = %target.nick, channel = %canonical_name, "User left channel");

    Ok(true)
}
