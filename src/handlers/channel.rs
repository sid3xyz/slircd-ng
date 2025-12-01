//! Channel command handlers.
//!
//! Handles JOIN, PART, TOPIC, NAMES, KICK commands.

use super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_notonchannel,
    err_usernotinchannel, matches_hostmask, server_reply, user_prefix,
};
use crate::db::ChannelRepository;
use crate::security::{ExtendedBan, UserContext, matches_extended_ban};
use crate::state::{Channel, ListEntry, MemberModes, Topic, User};
use async_trait::async_trait;
use slirc_proto::{
    ChannelExt, ChannelMode, Command, Message, MessageRef, Mode, Prefix, Response, irc_to_lower,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Check if a ban entry matches a user, supporting both hostmask and extended bans.
fn matches_ban(entry: &ListEntry, user_mask: &str, user_context: &UserContext) -> bool {
    if entry.mask.starts_with('$') {
        // Extended ban format ($a:account, $r:realname, etc.)
        if let Some(extban) = ExtendedBan::parse(&entry.mask) {
            matches_extended_ban(&extban, user_context)
        } else {
            false
        }
    } else {
        // Traditional nick!user@host ban
        matches_hostmask(&entry.mask, user_mask)
    }
}

/// Handler for JOIN command.
pub struct JoinHandler;

#[async_trait]
impl Handler for JoinHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // JOIN <channels> [keys]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        // Handle "JOIN 0" - leave all channels
        if channels_str == "0" {
            return leave_all_channels(ctx).await;
        }

        // Check join rate limit before processing any channels
        let uid_string = ctx.uid.to_string();
        if !ctx.matrix.rate_limiter.check_join_rate(&uid_string) {
            let nick = ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string());
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_TOOMANYCHANNELS,
                vec![
                    nick,
                    channels_str.to_string(),
                    "You are joining channels too quickly. Please wait.".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Parse channel list (comma-separated)
        for channel_name in channels_str.split(',') {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            if !channel_name.is_channel_name() {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.handshake
                            .nick
                            .clone()
                            .unwrap_or_else(|| "*".to_string()),
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
    let realname = ctx
        .handshake
        .realname
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;

    // Build user's full mask for ban/invite checks (nick!user@host)
    let host = ctx.remote_addr.ip().to_string();
    let user_mask = format!("{}!{}@{}", nick, user_name, host);

    // Build UserContext for extended ban checks
    let user_context = UserContext::for_registration(
        ctx.remote_addr.ip(),
        host.clone(),
        nick.clone(),
        user_name.clone(),
        realname.clone(),
        ctx.matrix.server_info.name.clone(),
        ctx.handshake.account.clone(),
    );

    // Check AKICK before joining
    if let Some(akick) = check_akick(ctx, &channel_lower, nick, user_name).await {
        // User is on the AKICK list - send a notice and refuse join
        let reason = akick
            .reason
            .as_deref()
            .unwrap_or("You are banned from this channel");
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::NOTICE(
                nick.clone(),
                format!(
                    "You are not permitted to be on \x02{}\x02: {}",
                    channel_name, reason
                ),
            ),
        };
        ctx.sender.send(notice).await?;
        info!(nick = %nick, channel = %channel_name, mask = %akick.mask, "AKICK triggered");
        return Ok(());
    }

    // Get or create channel
    let channel = ctx
        .matrix
        .channels
        .entry(channel_lower.clone())
        .or_insert_with(|| Arc::new(RwLock::new(Channel::new(channel_name.to_string()))))
        .clone();

    let mut channel_guard = channel.write().await;

    // Check if already in channel
    if channel_guard.is_member(ctx.uid) {
        return Ok(());
    }

    // Enforce channel access controls:

    // 1. Check Invite-Only (+i) mode
    if channel_guard.modes.invite_only {
        // Check invite exception list (+I) - supports extended bans
        let has_invex = channel_guard
            .invex
            .iter()
            .any(|entry| matches_ban(entry, &user_mask, &user_context));

        // TODO: Check if user is in temporary invite list (from INVITE command)
        // For now, only check invex

        if !has_invex {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_INVITEONLYCHAN,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "Cannot join channel (+i)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: invite-only");
            return Ok(());
        }
    }

    // 2. Check ban list (+b) and ban exceptions (+e) - supports extended bans
    let is_banned = channel_guard
        .bans
        .iter()
        .any(|entry| matches_ban(entry, &user_mask, &user_context));

    if is_banned {
        // Check if user has ban exception (+e) - supports extended bans
        let has_exception = channel_guard
            .excepts
            .iter()
            .any(|entry| matches_ban(entry, &user_mask, &user_context));

        if !has_exception {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_BANNEDFROMCHAN,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "Cannot join channel (+b)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: banned");
            return Ok(());
        }
    }

    // Access checks passed, proceed with join

    // Determine member modes:
    // 1. First user gets ops
    // 2. If channel is registered, check access list for auto-op/voice
    let modes = if channel_guard.members.is_empty() {
        MemberModes {
            op: true,
            voice: false,
        }
    } else {
        // Only check for auto-op/voice if channel is registered
        if ctx.matrix.registered_channels.contains(&channel_lower) {
            check_auto_modes(ctx, &channel_lower)
                .await
                .unwrap_or_default()
        } else {
            MemberModes::default()
        }
    };

    // Add user to channel
    channel_guard.add_member(ctx.uid.to_string(), modes.clone());
    let member_count = channel_guard.members.len();
    let topic = channel_guard.topic.clone();
    let canonical_name = channel_guard.name.clone();

    drop(channel_guard);

    // Add channel to user's list
    if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    } else {
        // User doesn't exist in matrix yet, create them
        let security_config = &ctx.matrix.config.security;
        let user = User::new(
            ctx.uid.to_string(),
            nick.clone(),
            user_name.clone(),
            realname.clone(),
            "localhost".to_string(),
            &security_config.cloak_secret,
            &security_config.cloak_suffix,
        );
        let user = Arc::new(RwLock::new(user));
        ctx.matrix.users.insert(ctx.uid.to_string(), user.clone());
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    }

    // Broadcast JOIN to all channel members (including self)
    // Use extended-join format if capability is enabled
    let join_msg = if ctx.handshake.capabilities.contains("extended-join") {
        // Get account name for extended join
        let account = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.account.clone()
        } else {
            None
        };
        let account_name = account.as_deref().unwrap_or("*");

        Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::JOIN(
                canonical_name.clone(),
                Some(account_name.to_string()),
                Some(realname.clone()),
            ),
        }
    } else {
        Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::JOIN(canonical_name.clone(), None, None),
        }
    };

    ctx.matrix
        .broadcast_to_channel(&channel_lower, join_msg, None)
        .await;

    // If auto-op/voice was applied, broadcast the MODE change using typed modes
    if modes.op || modes.voice {
        let mut mode_changes: Vec<Mode<ChannelMode>> = Vec::new();

        if modes.op {
            mode_changes.push(Mode::plus(ChannelMode::Oper, Some(nick)));
        }
        if modes.voice {
            mode_changes.push(Mode::plus(ChannelMode::Voice, Some(nick)));
        }

        let mode_str = mode_changes
            .iter()
            .map(|m| m.flag())
            .collect::<String>();

        let mode_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::ChannelMODE(canonical_name.clone(), mode_changes),
        };

        ctx.matrix
            .broadcast_to_channel(&channel_lower, mode_msg, None)
            .await;
        info!(nick = %nick, channel = %canonical_name, modes = %mode_str, "Auto-modes applied");
    }

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

/// Check if user should receive auto-op or auto-voice on a registered channel.
/// Returns Some(MemberModes) if the user has access, None otherwise.
async fn check_auto_modes(ctx: &Context<'_>, channel_lower: &str) -> Option<MemberModes> {
    // Get user's account name if identified
    let account_name = {
        let user = ctx.matrix.users.get(ctx.uid)?;
        let user = user.read().await;

        if !user.modes.registered {
            return None;
        }

        user.account.clone()?
    };

    // Look up account ID
    let account = ctx.db.accounts().find_by_name(&account_name).await.ok()??;

    // Look up channel record
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

    // Check if user is founder
    if account.id == channel_record.founder_account_id {
        return Some(MemberModes {
            op: true,
            voice: false,
        });
    }

    // Check access list
    let access = ctx
        .db
        .channels()
        .get_access(channel_record.id, account.id)
        .await
        .ok()??;

    let op = ChannelRepository::has_op_access(&access.flags);
    let voice = ChannelRepository::has_voice_access(&access.flags);

    if op || voice {
        Some(MemberModes { op, voice })
    } else {
        None
    }
}

/// Check if user is on the AKICK list for a channel.
/// Returns the matching AKICK entry if found.
async fn check_akick(
    ctx: &Context<'_>,
    channel_lower: &str,
    nick: &str,
    user: &str,
) -> Option<crate::db::ChannelAkick> {
    // Look up channel record
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

    // Get host - for now use "localhost" but this should be the actual host
    let host = "localhost";

    // Check AKICK list
    ctx.db
        .channels()
        .check_akick(channel_record.id, nick, user, host)
        .await
        .ok()?
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_>) -> HandlerResult {
    let nick = ctx
        .handshake
        .nick
        .clone()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let user_name = ctx
        .handshake
        .user
        .clone()
        .ok_or(HandlerError::NickOrUserMissing)?;

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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // PART <channels> [reason]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(1);

        let nick = ctx
            .handshake
            .nick
            .clone()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx
            .handshake
            .user
            .clone()
            .ok_or(HandlerError::NickOrUserMissing)?;

        for channel_name in channels_str.split(',') {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);
            leave_channel_internal(ctx, &channel_lower, &nick, &user_name, reason).await?;
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
        ctx.sender
            .send(err_notonchannel(
                &ctx.matrix.server_info.name,
                nick,
                &channel_guard.name,
            ))
            .await?;
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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // TOPIC <channel> [new_topic]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let new_topic = msg.arg(1);

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
        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        nick.to_string(),
                        channel_name.to_string(),
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
            ctx.sender
                .send(err_notonchannel(
                    &ctx.matrix.server_info.name,
                    nick,
                    &channel_guard.name,
                ))
                .await?;
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
                    text: topic_text.to_string(),
                    set_by: format!("{}!{}@localhost", nick, user_name),
                    set_at: chrono::Utc::now().timestamp(),
                };
                channel_guard.topic = Some(new_topic);

                // Broadcast topic change to channel
                let topic_msg = Message {
                    tags: None,
                    prefix: Some(user_prefix(nick, user_name, "localhost")),
                    command: Command::TOPIC(canonical_name.clone(), Some(topic_text.to_string())),
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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // NAMES [channel [target]]
        let channel_name = msg.arg(0).unwrap_or("");

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        if channel_name.is_empty() {
            // NAMES without channel - not implemented
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    "*".to_string(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(channel_name);

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
                    channel_name.to_string(),
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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // KICK <channel> <nick> [reason]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target_nick = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(2);

        if channel_name.is_empty() || target_nick.is_empty() {
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
        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        nick.clone(),
                        channel_name.to_string(),
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let mut channel_guard = channel.write().await;

        // Check if kicker is op
        if !channel_guard.is_op(ctx.uid) {
            ctx.sender
                .send(err_chanoprivsneeded(
                    &ctx.matrix.server_info.name,
                    nick,
                    &channel_guard.name,
                ))
                .await?;
            return Ok(());
        }

        // Find target user
        let target_lower = irc_to_lower(target_nick);
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        "No such nick".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if target is in channel
        if !channel_guard.is_member(&target_uid) {
            ctx.sender
                .send(err_usernotinchannel(
                    &ctx.matrix.server_info.name,
                    nick,
                    target_nick,
                    &channel_guard.name,
                ))
                .await?;
            return Ok(());
        }

        let canonical_name = channel_guard.name.clone();
        let kick_reason = reason
            .map(|s| s.to_string())
            .unwrap_or_else(|| nick.clone());

        // Broadcast KICK to channel (before removing)
        let kick_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::KICK(
                canonical_name.clone(),
                target_nick.to_string(),
                Some(kick_reason),
            ),
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
