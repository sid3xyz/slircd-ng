//! JOIN command handler and related functionality.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, server_reply, user_prefix, with_label,
};
use super::matches_ban;
use crate::db::ChannelRepository;
use crate::security::UserContext;
use crate::state::{Channel, MemberModes, User};
use async_trait::async_trait;
use slirc_proto::{
    ChannelExt, ChannelMode, Command, Message, MessageRef, Mode, Prefix, Response, irc_to_lower,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

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
        .or_insert_with(|| {
            crate::metrics::ACTIVE_CHANNELS.inc();
            Arc::new(RwLock::new(Channel::new(channel_name.to_string())))
        })
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

    // 1b. Check Registered-Only (+r) mode
    if channel_guard.modes.registered_only {
        // Determine if the user is identified (user mode +r)
        let is_registered = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.modes.registered
        } else {
            false
        };

        if !is_registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NEEDREGGEDNICK,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "Cannot join channel (+r)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            crate::metrics::REGISTERED_ONLY_BLOCKED.inc();
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: +r registered-only");
            return Ok(());
        }
    }

    // 2. Check ban list (+b) and ban exceptions (+e) - supports extended bans
    let is_banned = channel_guard
        .bans
        .iter()
        .any(|entry| matches_ban(entry, &user_mask, &user_context))
        || channel_guard
            .extended_bans
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
            crate::metrics::BANS_TRIGGERED.inc();
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: banned");
            return Ok(());
        }
    }

    // Access checks passed, proceed with join

    // Determine member modes:
    // 1. First user gets ops
    // 2. If channel is registered, check access list for auto-op/voice
    let now = chrono::Utc::now().timestamp();
    let modes = if channel_guard.members.is_empty() {
        MemberModes {
            op: true,
            voice: false,
            join_time: Some(now),
        }
    } else {
        // Only check for auto-op/voice if channel is registered
        if ctx.matrix.registered_channels.contains(&channel_lower) {
            let mut member_modes = check_auto_modes(ctx, &channel_lower)
                .await
                .unwrap_or_default();
            member_modes.join_time = Some(now);
            member_modes
        } else {
            MemberModes {
                join_time: Some(now),
                ..Default::default()
            }
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
            ctx.handshake.capabilities.clone(),
        );
        let user = Arc::new(RwLock::new(user));
        ctx.matrix.users.insert(ctx.uid.to_string(), user.clone());
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    }

    // Broadcast JOIN to all channel members (including self)
    // For extended-join: recipients with the capability get the extended format,
    // recipients without get the standard format
    let account = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        user.account.clone()
    } else {
        None
    };
    let account_name = account.as_deref().unwrap_or("*");

    // Extended JOIN format: :nick!user@host JOIN #channel account :realname
    let extended_join_msg = Message {
        tags: None,
        prefix: Some(user_prefix(nick, user_name, "localhost")),
        command: Command::JOIN(
            canonical_name.clone(),
            Some(account_name.to_string()),
            Some(realname.clone()),
        ),
    };

    // Standard JOIN format: :nick!user@host JOIN #channel
    let standard_join_msg = Message {
        tags: None,
        prefix: Some(user_prefix(nick, user_name, "localhost")),
        command: Command::JOIN(canonical_name.clone(), None, None),
    };

    ctx.matrix
        .broadcast_to_channel_with_cap(
            &channel_lower,
            extended_join_msg,
            None,
            Some("extended-join"),
            Some(standard_join_msg),
        )
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

    let end_names = with_label(
        server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_ENDOFNAMES,
            vec![
                nick.clone(),
                canonical_name,
                "End of /NAMES list".to_string(),
            ],
        ),
        ctx.label.as_deref(),
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
            join_time: None, // Will be set by caller
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
        Some(MemberModes { op, voice, join_time: None }) // join_time set by caller
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
        super::part::leave_channel_internal(ctx, &channel_lower, &nick, &user_name, None).await?;
    }

    Ok(())
}
