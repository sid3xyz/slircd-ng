//! Channel command handlers.
//!
//! Handles JOIN, PART, TOPIC, NAMES, KICK commands.

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use crate::state::{Channel, MemberModes, Topic, User};
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Prefix, Response};

/// Helper to create a user prefix.
fn user_prefix(nick: &str, user: &str, host: &str) -> Prefix {
    Prefix::Nickname(nick.to_string(), user.to_string(), host.to_string())
}
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Validates a channel name per RFC 2811/2812.
/// Channel names start with '#', '&', '+', or '!' and cannot contain
/// spaces, commas, or ^G (BEL).
fn is_valid_channel(name: &str) -> bool {
    if name.is_empty() || name.len() > 50 {
        return false;
    }
    // Must start with #, &, +, or ! (RFC 2811)
    let first = name.chars().next().unwrap();
    if !matches!(first, '#' | '&' | '+' | '!') {
        return false;
    }
    // No spaces, commas, NUL, or BEL (^G) per RFC 2812
    name.chars().skip(1).all(|c| c != ' ' && c != ',' && c != '\x07' && c != '\0' && c.is_ascii())
}

/// Helper to create a message with user prefix.
#[allow(dead_code)]
fn user_message(user: &User, command: Command) -> Message {
    Message {
        tags: None,
        prefix: Some(user_prefix(&user.nick, &user.user, &user.host)),
        command,
    }
}

/// Handler for JOIN command.
pub struct JoinHandler;

#[async_trait]
impl Handler for JoinHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let channels_str = match &msg.command {
            Command::JOIN(c, _, _) => c.clone(),
            _ => return Ok(()),
        };

        // Handle "JOIN 0" - leave all channels
        if channels_str == "0" {
            return leave_all_channels(ctx).await;
        }

        // Parse channel list (comma-separated)
        for channel_name in channels_str.split(',') {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            if !is_valid_channel(channel_name) {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                        channel_name.to_string(),
                        "Invalid channel name".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                continue;
            }

            join_channel(ctx, channel_name).await?;
        }

        Ok(())
    }
}

/// Join a single channel.
async fn join_channel(ctx: &mut Context<'_>, channel_name: &str) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);
    let nick = ctx.handshake.nick.as_ref().unwrap();
    let user_name = ctx.handshake.user.as_ref().unwrap();
    let realname = ctx.handshake.realname.as_ref().unwrap();

    // Get or create channel
    let channel = ctx.matrix.channels.entry(channel_lower.clone()).or_insert_with(|| {
        Arc::new(RwLock::new(Channel::new(channel_name.to_string())))
    }).clone();

    let mut channel_guard = channel.write().await;

    // Check if already in channel
    if channel_guard.is_member(ctx.uid) {
        return Ok(());
    }

    // First user gets ops
    let modes = if channel_guard.members.is_empty() {
        MemberModes { op: true, voice: false }
    } else {
        MemberModes::default()
    };

    // Add user to channel
    channel_guard.add_member(ctx.uid.to_string(), modes);
    let member_count = channel_guard.members.len();
    let topic = channel_guard.topic.clone();
    let canonical_name = channel_guard.name.clone();

    // Build NAMES list before releasing lock
    let mut names_list = Vec::new();
    for (uid, member_modes) in &channel_guard.members {
        if let Some(user_uid) = ctx.matrix.nicks.iter().find(|e| e.value() == uid) {
            let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                format!("{}{}", prefix, user_uid.key())
            } else {
                // Get nick from uid
                if let Some(u) = ctx.matrix.users.get(uid) {
                    let u = u.read().await;
                    u.nick.clone()
                } else {
                    continue;
                }
            };
            names_list.push(nick_with_prefix);
        }
    }

    drop(channel_guard);

    // Add channel to user's list
    if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    } else {
        // User doesn't exist in matrix yet, create them
        let user = User::new(
            ctx.uid.to_string(),
            nick.clone(),
            user_name.clone(),
            realname.clone(),
            "localhost".to_string(),
        );
        let user = Arc::new(RwLock::new(user));
        ctx.matrix.users.insert(ctx.uid.to_string(), user.clone());
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    }

    // Broadcast JOIN to all channel members (including self)
    let join_msg = Message {
        tags: None,
        prefix: Some(user_prefix(nick, user_name, "localhost")),
        command: Command::JOIN(canonical_name.clone(), None, None),
    };

    ctx.matrix.broadcast_to_channel(&channel_lower, join_msg, None).await;

    info!(nick = %nick, channel = %canonical_name, members = member_count, "User joined channel");

    // Send topic if set
    if let Some(topic) = topic {
        let topic_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_TOPIC,
            vec![nick.clone(), canonical_name.clone(), topic.text],
        );
        ctx.sender.send(topic_reply).await?;
    }

    // Send NAMES list
    // Rebuild names list with correct nicks
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
    drop(channel);

    let names_reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::RPL_NAMREPLY,
        vec![
            nick.clone(),
            "=".to_string(), // public channel
            canonical_name.clone(),
            names_list.join(" "),
        ],
    );
    ctx.sender.send(names_reply).await?;

    let end_names = server_reply(
        &ctx.matrix.server_info.name,
        Response::RPL_ENDOFNAMES,
        vec![
            nick.clone(),
            canonical_name,
            "End of /NAMES list".to_string(),
        ],
    );
    ctx.sender.send(end_names).await?;

    Ok(())
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_>) -> HandlerResult {
    let nick = ctx.handshake.nick.clone().unwrap();
    let user_name = ctx.handshake.user.clone().unwrap();

    // Get list of channels user is in
    let channels: Vec<String> = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let user = user.read().await;
        user.channels.iter().cloned().collect()
    } else {
        return Ok(());
    };

    // Leave each channel
    for channel_lower in channels {
        leave_channel_internal(ctx, &channel_lower, &nick, &user_name, None).await?;
    }

    Ok(())
}

/// Handler for PART command.
pub struct PartHandler;

#[async_trait]
impl Handler for PartHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let (channels_str, reason) = match &msg.command {
            Command::PART(c, r) => (c.clone(), r.clone()),
            _ => return Ok(()),
        };

        let nick = ctx.handshake.nick.clone().unwrap();
        let user_name = ctx.handshake.user.clone().unwrap();

        for channel_name in channels_str.split(',') {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);
            leave_channel_internal(ctx, &channel_lower, &nick, &user_name, reason.as_deref()).await?;
        }

        Ok(())
    }
}

/// Internal function to leave a channel.
async fn leave_channel_internal(
    ctx: &mut Context<'_>,
    channel_lower: &str,
    nick: &str,
    user_name: &str,
    reason: Option<&str>,
) -> HandlerResult {
    // Check if channel exists
    let channel = match ctx.matrix.channels.get(channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    nick.to_string(),
                    channel_lower.to_string(),
                    "No such channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }
    };

    let mut channel_guard = channel.write().await;

    // Check if user is in channel
    if !channel_guard.is_member(ctx.uid) {
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::ERR_NOTONCHANNEL,
            vec![
                nick.to_string(),
                channel_guard.name.clone(),
                "You're not on that channel".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(());
    }

    let canonical_name = channel_guard.name.clone();

    // Broadcast PART before removing
    let part_msg = Message {
        tags: None,
        prefix: Some(user_prefix(nick, user_name, "localhost")),
        command: Command::PART(canonical_name.clone(), reason.map(String::from)),
    };

    // Broadcast to all members including self
    for uid in channel_guard.members.keys() {
        if let Some(sender) = ctx.matrix.senders.get(uid) {
            let _ = sender.send(part_msg.clone()).await;
        }
    }

    // Remove user from channel
    channel_guard.remove_member(ctx.uid);
    let is_empty = channel_guard.members.is_empty();

    drop(channel_guard);

    // Remove channel from user's list
    if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let mut user = user.write().await;
        user.channels.remove(channel_lower);
    }

    // If channel is now empty, remove it
    if is_empty {
        ctx.matrix.channels.remove(channel_lower);
        debug!(channel = %canonical_name, "Channel removed (empty)");
    }

    info!(nick = %nick, channel = %canonical_name, "User left channel");

    Ok(())
}

/// Handler for TOPIC command.
pub struct TopicHandler;

#[async_trait]
impl Handler for TopicHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let (channel_name, new_topic) = match &msg.command {
            Command::TOPIC(c, t) => (c.clone(), t.clone()),
            _ => return Ok(()),
        };

        let nick = ctx.handshake.nick.as_ref().unwrap();
        let user_name = ctx.handshake.user.as_ref().unwrap();
        let channel_lower = irc_to_lower(&channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        nick.to_string(),
                        channel_name,
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let mut channel_guard = channel.write().await;

        // Check if user is in channel
        if !channel_guard.is_member(ctx.uid) {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTONCHANNEL,
                vec![
                    nick.to_string(),
                    channel_guard.name.clone(),
                    "You're not on that channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let canonical_name = channel_guard.name.clone();

        match new_topic {
            None => {
                // Query topic
                match &channel_guard.topic {
                    Some(topic) => {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_TOPIC,
                            vec![nick.to_string(), canonical_name.clone(), topic.text.clone()],
                        );
                        ctx.sender.send(reply).await?;

                        let who_reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_TOPICWHOTIME,
                            vec![
                                nick.to_string(),
                                canonical_name,
                                topic.set_by.clone(),
                                topic.set_at.to_string(),
                            ],
                        );
                        ctx.sender.send(who_reply).await?;
                    }
                    None => {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_NOTOPIC,
                            vec![
                                nick.to_string(),
                                canonical_name,
                                "No topic is set".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
                }
            }
            Some(topic_text) => {
                // Set topic (for now, anyone can set - add mode checks later)
                let new_topic = Topic {
                    text: topic_text.clone(),
                    set_by: format!("{}!{}@localhost", nick, user_name),
                    set_at: chrono::Utc::now().timestamp(),
                };
                channel_guard.topic = Some(new_topic);

                // Broadcast topic change to channel
                let topic_msg = Message {
                    tags: None,
                    prefix: Some(user_prefix(nick, user_name, "localhost")),
                    command: Command::TOPIC(canonical_name.clone(), Some(topic_text)),
                };

                for uid in channel_guard.members.keys() {
                    if let Some(sender) = ctx.matrix.senders.get(uid) {
                        let _ = sender.send(topic_msg.clone()).await;
                    }
                }

                info!(nick = %nick, channel = %canonical_name, "Topic changed");
            }
        }

        Ok(())
    }
}

/// Handler for NAMES command.
pub struct NamesHandler;

#[async_trait]
impl Handler for NamesHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // NAMES is sent as Raw since it might not be in Command enum
        let channel_name = match &msg.command {
            Command::Raw(cmd, params) if cmd.eq_ignore_ascii_case("NAMES") => {
                params.first().cloned().unwrap_or_default()
            }
            _ => return Ok(()),
        };

        let nick = ctx.handshake.nick.as_ref().unwrap();

        if channel_name.is_empty() {
            // NAMES without channel - not implemented
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![nick.to_string(), "*".to_string(), "End of /NAMES list".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(&channel_name);

        if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
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

            let names_reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_NAMREPLY,
                vec![
                    nick.to_string(),
                    "=".to_string(),
                    channel.name.clone(),
                    names_list.join(" "),
                ],
            );
            ctx.sender.send(names_reply).await?;

            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel.name.clone(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        } else {
            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel_name,
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        }

        Ok(())
    }
}

/// Handler for KICK command.
pub struct KickHandler;

#[async_trait]
impl Handler for KickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let (channel_name, target_nick, reason) = match &msg.command {
            Command::KICK(c, t, r) => (c.clone(), t.clone(), r.clone()),
            _ => return Ok(()),
        };

        if channel_name.is_empty() || target_nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx.handshake.nick.as_ref().unwrap();
        let user_name = ctx.handshake.user.as_ref().unwrap();
        let channel_lower = irc_to_lower(&channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![nick.clone(), channel_name, "No such channel".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let mut channel_guard = channel.write().await;

        // Check if kicker is op
        if !channel_guard.is_op(ctx.uid) {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_CHANOPRIVSNEEDED,
                vec![
                    nick.clone(),
                    channel_guard.name.clone(),
                    "You're not channel operator".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Find target user
        let target_lower = irc_to_lower(&target_nick);
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![nick.clone(), target_nick, "No such nick".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if target is in channel
        if !channel_guard.is_member(&target_uid) {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_USERNOTINCHANNEL,
                vec![
                    nick.clone(),
                    target_nick,
                    channel_guard.name.clone(),
                    "They aren't on that channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let canonical_name = channel_guard.name.clone();
        let kick_reason = reason.unwrap_or_else(|| nick.clone());

        // Broadcast KICK to channel (before removing)
        let kick_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::KICK(canonical_name.clone(), target_nick.clone(), Some(kick_reason)),
        };

        for uid in channel_guard.members.keys() {
            if let Some(sender) = ctx.matrix.senders.get(uid) {
                let _ = sender.send(kick_msg.clone()).await;
            }
        }

        // Remove target from channel
        channel_guard.remove_member(&target_uid);

        drop(channel_guard);

        // Remove channel from target's list
        if let Some(user) = ctx.matrix.users.get(&target_uid) {
            let mut user = user.write().await;
            user.channels.remove(&channel_lower);
        }

        info!(
            kicker = %nick,
            target = %target_nick,
            channel = %canonical_name,
            "User kicked from channel"
        );

        Ok(())
    }
}
