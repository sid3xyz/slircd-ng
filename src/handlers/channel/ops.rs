//! Channel membership operations.
//!
//! Used by both regular JOIN/PART and admin SA* commands.
//! These functions perform the core channel membership operations without
//! permission checks, allowing callers to implement their own access control.

use super::super::{Context, HandlerError, HandlerResult, server_reply, with_label};
use crate::state::MemberModes;
use slirc_proto::{Command, Message, Prefix, Response, irc_to_lower};
use tracing::info;

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
pub async fn force_join_channel<S>(
    ctx: &Context<'_, S>,
    target: &TargetUser<'_>,
    channel_name: &str,
    modes: MemberModes,
    send_topic_names_to: Option<&tokio::sync::mpsc::Sender<Message>>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);
    let mailbox_capacity = ctx.matrix.config.limits.channel_mailbox_capacity;

    // Get or create channel
    let observer = ctx.matrix.channel_manager.observer.clone();
    let channel_ref = ctx
        .matrix
        .channel_manager
        .channels
        .entry(channel_lower.clone())
        .or_insert_with(|| {
            crate::metrics::ACTIVE_CHANNELS.inc();
            crate::state::actor::ChannelActor::spawn_with_capacity(
                channel_name.to_string(),
                std::sync::Arc::downgrade(ctx.matrix),
                None, // No initial topic for SAJOIN-created channels
                mailbox_capacity,
                observer,
            )
        })
        .clone();

    // Get user data
    let (caps, user_context, sender, session_id) =
        if let Some(user_arc) = ctx.matrix.user_manager.users.get(target.uid).map(|u| u.value().clone()) {
            let user = user_arc.read().await;
            let context = crate::security::UserContext::for_registration(
                crate::security::RegistrationParams {
                    hostname: user.host.clone(),
                    nickname: user.nick.clone(),
                    username: user.user.clone(),
                    realname: user.realname.clone(),
                    server: ctx.server_name().to_string(),
                    account: user.account.clone(),
                    is_tls: user.modes.secure,
                    is_oper: user.modes.oper,
                    oper_type: user.modes.oper_type.clone(),
                },
            );
            let sender = ctx.matrix.user_manager.senders.get(target.uid).map(|s| s.clone());
            (user.caps.clone(), context, sender, user.session_id)
        } else {
            return Ok(());
        };

    let Some(sender) = sender else {
        return Ok(());
    };

    // Prepare messages
    let prefix = Some(Prefix::new(
        target.nick.to_string(),
        target.user.to_string(),
        target.host.to_string(),
    ));

    let join_msg_standard = Message {
        tags: None,
        prefix: prefix.clone(),
        command: Command::JOIN(channel_name.to_string(), None, None),
    };

    let account = user_context.account.as_deref().unwrap_or("*");
    let join_msg_extended = Message {
        tags: None,
        prefix: prefix.clone(),
        command: Command::JOIN(
            channel_name.to_string(),
            Some(account.to_string()),
            Some(user_context.realname.clone()),
        ),
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    let event = crate::state::actor::ChannelEvent::Join {
        params: Box::new(crate::state::actor::JoinParams {
            uid: target.uid.to_string(),
            nick: target.nick.to_string(),
            sender: sender.clone(),
            caps,
            user_context,
            key: None,
            initial_modes: Some(modes),
            join_msg_extended,
            join_msg_standard,
            session_id,
        }),
        reply_tx,
    };

    if (channel_ref.send(event).await).is_err() {
        return Err(HandlerError::Internal("Channel actor died".to_string()));
    }

    let join_data = match reply_rx.await {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => return Err(HandlerError::Internal(e.to_string())),
        Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
    };

    // Add channel to user's list
    if let Some(user_arc) = ctx.matrix.user_manager.users.get(target.uid).map(|u| u.value().clone()) {
        let mut user = user_arc.write().await;
        user.channels.insert(channel_lower.clone());
    }

    info!(
        nick = %target.nick,
        channel = %join_data.channel_name,
        "User joined channel"
    );

    // Send topic and NAMES to joining user if requested
    if let Some(sender) = send_topic_names_to {
        // Send topic if set
        if let Some(topic) = join_data.topic {
            let topic_reply = server_reply(
                ctx.server_name(),
                Response::RPL_TOPIC,
                vec![
                    target.nick.to_string(),
                    join_data.channel_name.clone(),
                    topic.text,
                ],
            );
            sender.send(topic_reply).await?;
        }

        // Send NAMES using GetMembers (oneshot-based, no deadlock)
        let (members_tx, members_rx) = tokio::sync::oneshot::channel();
        let _ = channel_ref
            .send(crate::state::actor::ChannelEvent::GetMembers {
                reply_tx: members_tx,
            })
            .await;

        if let Ok(members) = members_rx.await {
            let channel_symbol = if join_data.is_secret { "@" } else { "=" };
            let mut names_list = Vec::with_capacity(members.len());

            for (uid, member_modes) in members {
                if let Some(user_arc) = ctx.matrix.user_manager.users.get(&uid).map(|u| u.value().clone()) {
                    let user = user_arc.read().await;
                    let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                        format!("{}{}", prefix, user.nick)
                    } else {
                        user.nick.clone()
                    };
                    names_list.push(nick_with_prefix);
                }
            }

            let names_reply = server_reply(
                ctx.server_name(),
                Response::RPL_NAMREPLY,
                vec![
                    target.nick.to_string(),
                    channel_symbol.to_string(),
                    join_data.channel_name.clone(),
                    names_list.join(" "),
                ],
            );
            sender.send(names_reply).await?;
        }

        let end_names = with_label(
            server_reply(
                ctx.server_name(),
                Response::RPL_ENDOFNAMES,
                vec![
                    target.nick.to_string(),
                    join_data.channel_name.clone(),
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
pub async fn force_part_channel<S>(
    ctx: &Context<'_, S>,
    target: &TargetUser<'_>,
    channel_lower: &str,
    reason: Option<&str>,
) -> Result<bool, super::super::HandlerError> {
    // Get channel reference
    let channel_sender = match ctx.matrix.channel_manager.channels.get(channel_lower) {
        Some(c) => c.clone(),
        None => return Ok(false),
    };

    let prefix = slirc_proto::Prefix::new(
        target.nick.to_string(),
        target.user.to_string(),
        target.host.to_string(),
    );

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let event = crate::state::actor::ChannelEvent::Part {
        uid: target.uid.to_string(),
        reason: reason.map(|s| s.to_string()),
        prefix,
        reply_tx,
    };

    if (channel_sender.send(event).await).is_err() {
        // Channel actor died, remove it
        ctx.matrix.channel_manager.channels.remove(channel_lower);
        return Ok(false);
    }

    match reply_rx.await {
        Ok(Ok(remaining_members)) => {
            // Success
            // Remove channel from user's list
            if let Some(user) = ctx.matrix.user_manager.users.get(target.uid) {
                let mut user = user.write().await;
                user.channels.remove(channel_lower);
            }

            if remaining_members == 0 {
                ctx.matrix.channel_manager.channels.remove(channel_lower);
                crate::metrics::ACTIVE_CHANNELS.dec();
            }

            Ok(true)
        }
        Ok(Err(_)) => Ok(false), // User not in channel
        Err(_) => Ok(false),     // Actor dropped
    }
}
