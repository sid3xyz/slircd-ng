//! JOIN command handler and related functionality.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, server_reply, user_mask_from_state, user_prefix,
    with_label,
};
use super::matches_ban;
use crate::db::ChannelRepository;
use crate::security::UserContext;
use crate::state::{Channel, MemberModes, User, parse_mlock};
use async_trait::async_trait;
use slirc_proto::{
    ChannelExt, ChannelMode, Command, Message, MessageRef, Mode, Prefix, Response, irc_to_lower,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Handler for JOIN command.
pub struct JoinHandler;

/// Apply MLOCK modes to a channel's mode struct.
///
/// MLOCK (mode lock) ensures certain modes are always set or unset on a channel.
/// This is typically configured via ChanServ and enforced when the channel is created.
fn apply_mlock_to_channel(channel: &mut Channel, modes: &[Mode<ChannelMode>]) {
    for mode in modes {
        match mode {
            // Simple flags (Type D - no parameters)
            Mode::Plus(ChannelMode::NoExternalMessages, _) => channel.modes.no_external = true,
            Mode::Minus(ChannelMode::NoExternalMessages, _) => channel.modes.no_external = false,
            Mode::Plus(ChannelMode::ProtectedTopic, _) => channel.modes.topic_lock = true,
            Mode::Minus(ChannelMode::ProtectedTopic, _) => channel.modes.topic_lock = false,
            Mode::Plus(ChannelMode::Secret, _) => channel.modes.secret = true,
            Mode::Minus(ChannelMode::Secret, _) => channel.modes.secret = false,
            Mode::Plus(ChannelMode::InviteOnly, _) => channel.modes.invite_only = true,
            Mode::Minus(ChannelMode::InviteOnly, _) => channel.modes.invite_only = false,
            Mode::Plus(ChannelMode::Moderated, _) => channel.modes.moderated = true,
            Mode::Minus(ChannelMode::Moderated, _) => channel.modes.moderated = false,
            Mode::Plus(ChannelMode::RegisteredOnly, _) => channel.modes.registered_only = true,
            Mode::Minus(ChannelMode::RegisteredOnly, _) => channel.modes.registered_only = false,
            Mode::Plus(ChannelMode::NoColors, _) => channel.modes.no_colors = true,
            Mode::Minus(ChannelMode::NoColors, _) => channel.modes.no_colors = false,
            Mode::Plus(ChannelMode::NoCTCP, _) => channel.modes.no_ctcp = true,
            Mode::Minus(ChannelMode::NoCTCP, _) => channel.modes.no_ctcp = false,
            Mode::Plus(ChannelMode::NoNickChange, _) => channel.modes.no_nick_change = true,
            Mode::Minus(ChannelMode::NoNickChange, _) => channel.modes.no_nick_change = false,
            // Parameter modes (Type B/C)
            Mode::Plus(ChannelMode::Key, Some(key)) => channel.modes.key = Some(key.clone()),
            Mode::Minus(ChannelMode::Key, _) => channel.modes.key = None,
            Mode::Plus(ChannelMode::Limit, Some(limit_str)) => {
                if let Ok(limit) = limit_str.parse::<u32>() {
                    channel.modes.limit = Some(limit);
                }
            }
            Mode::Minus(ChannelMode::Limit, _) => channel.modes.limit = None,
            // Skip list modes (bans, etc.) and prefix modes - not applicable for MLOCK
            _ => {}
        }
    }
}

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
            let nick = ctx
                .handshake
                .nick
                .clone()
                .unwrap_or_else(|| "*".to_string());
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

        // Parse channel list (comma-separated) and optional keys
        let channels: Vec<&str> = channels_str.split(',').collect();
        let keys: Vec<Option<&str>> = if let Some(keys_str) = msg.arg(1) {
            let mut key_list: Vec<Option<&str>> = keys_str
                .split(',')
                .map(|k| {
                    let trimmed = k.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                })
                .collect();
            // Pad with None if fewer keys than channels
            key_list.resize(channels.len(), None);
            key_list
        } else {
            vec![None; channels.len()]
        };

        for (i, channel_name) in channels.iter().enumerate() {
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

            let key = keys.get(i).and_then(|k| *k);
            join_channel(ctx, channel_name, key).await?;
        }

        Ok(())
    }
}

/// Join a single channel.
async fn join_channel(
    ctx: &mut Context<'_>,
    channel_name: &str,
    provided_key: Option<&str>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);
    let (nick, user_name, visible_host) = user_mask_from_state(ctx, ctx.uid)
        .await
        .ok_or(HandlerError::NickOrUserMissing)?;

    let (real_host, realname) = {
        let user_ref = ctx
            .matrix
            .users
            .get(ctx.uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (user.host.clone(), user.realname.clone())
    };

    // Build user's full mask for ban/invite checks (nick!user@real_host)
    let user_mask = format!("{}!{}@{}", nick, user_name, real_host);

    // Build UserContext for extended ban checks
    let ip_addr = ctx
        .handshake
        .webirc_ip
        .as_ref()
        .and_then(|ip| ip.parse().ok())
        .unwrap_or_else(|| ctx.remote_addr.ip());

    let user_context = UserContext::for_registration(
        ip_addr,
        real_host.clone(),
        nick.clone(),
        user_name.clone(),
        realname.clone(),
        ctx.matrix.server_info.name.clone(),
        ctx.handshake.account.clone(),
    );

    // Check AKICK before joining
    if let Some(akick) = check_akick(ctx, &channel_lower, &nick, &user_name).await {
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

    // Apply MLOCK for newly created registered channels
    // A channel is "new" if it has no members yet
    if channel_guard.members.is_empty() && ctx.matrix.registered_channels.contains(&channel_lower) {
        // Fetch the channel record to get MLOCK settings
        let mlock_str = ctx
            .db
            .channels()
            .find_by_name(&channel_lower)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.mlock);

        if let Some(mlock_str) = mlock_str {
            let mlock_modes = parse_mlock(&mlock_str);
            apply_mlock_to_channel(&mut channel_guard, &mlock_modes);
            if !mlock_modes.is_empty() {
                info!(channel = %channel_name, mlock = %mlock_str, "Applied MLOCK to new channel");
            }
        }
    }

    // Check if already in channel
    if channel_guard.is_member(ctx.uid) {
        return Ok(());
    }

    // Enforce channel access controls:

    // Check if user was invited via INVITE command (used by multiple checks below)
    let has_invite = channel_guard.invites.contains(ctx.uid);

    // 0. Check Channel Key (+k) mode
    if let Some(ref channel_key) = channel_guard.modes.key {
        // Channel has a key set - validate provided key
        if provided_key != Some(channel_key.as_str()) {
            // Key missing or incorrect
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_BADCHANNELKEY,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "Cannot join channel (+k)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: bad/missing key");
            return Ok(());
        }
    }

    // 1. Check Invite-Only (+i) mode
    if channel_guard.modes.invite_only {
        // Check invite exception list (+I) - supports extended bans
        let has_invex = channel_guard
            .invex
            .iter()
            .any(|entry| matches_ban(entry, &user_mask, &user_context));

        // Check if user was invited via INVITE command (has_invite computed above)

        if !has_invex && !has_invite {
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

    // 1c. Check Oper-Only (+O) mode
    if channel_guard.modes.oper_only {
        let is_oper = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            user.modes.oper
        } else {
            false
        };

        if !is_oper {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_INVITEONLYCHAN,
                vec![
                    nick.clone(),
                    channel_name.to_string(),
                    "Cannot join channel (+O) - Oper only".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            drop(channel_guard);
            info!(nick = %nick, channel = %channel_name, "JOIN denied: +O oper-only");
            return Ok(());
        }
    }

    // 1d. Check TLS-Only (+z) mode
    if channel_guard.modes.tls_only && !ctx.handshake.is_tls {
        let reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::ERR_SECUREONLYCHAN,
            vec![
                nick.clone(),
                channel_name.to_string(),
                "Cannot join channel (+z) - TLS connection required".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        drop(channel_guard);
        info!(nick = %nick, channel = %channel_name, "JOIN denied: +z TLS-only");
        return Ok(());
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

        // Being invited also exempts from bans
        if !has_exception && !has_invite {
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
    // 1. First user of an unregistered channel gets op (+o, shown as @)
    //    Note: We use op, not owner (~), because our PREFIX=(ov)@+ advertises only @ and +
    // 2. If channel is registered, check access list for auto-op/voice
    let now = chrono::Utc::now().timestamp();
    let modes = if channel_guard.members.is_empty() {
        MemberModes {
            owner: false,
            admin: false,
            op: true,
            halfop: false,
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
    // Remove from pending invites if they were invited
    channel_guard.invites.remove(ctx.uid);
    let member_count = channel_guard.members.len();
    let topic = channel_guard.topic.clone();
    let canonical_name = channel_guard.name.clone();

    drop(channel_guard);

    // Update last_used timestamp for registered channels (non-blocking)
    if ctx.matrix.registered_channels.contains(&channel_lower) {
        let db = ctx.db.clone();
        let channel_name = channel_lower.clone();
        tokio::spawn(async move {
            if let Err(e) = db.channels().touch_by_name(&channel_name).await {
                tracing::warn!(channel = %channel_name, error = ?e, "Failed to update channel last_used");
            }
        });
    }

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
            real_host.clone(),
            &security_config.cloak_secret,
            &security_config.cloak_suffix,
            ctx.handshake.capabilities.clone(),
            ctx.handshake.certfp.clone(),
        );
        let user = Arc::new(RwLock::new(user));
        ctx.matrix.users.insert(ctx.uid.to_string(), user.clone());
        let mut user = user.write().await;
        user.channels.insert(channel_lower.clone());
    }

    // Broadcast JOIN to all channel members (including self)
    // For extended-join: recipients with the capability get the extended format,
    // recipients without get the standard format
    let (account, away_message) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        (user.account.clone(), user.away.clone())
    } else {
        (None, None)
    };
    let account_name = account.as_deref().unwrap_or("*");

    // Extended JOIN format: :nick!user@host JOIN #channel account :realname
    let extended_join_msg = Message {
        tags: None,
        prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
        command: Command::JOIN(
            canonical_name.clone(),
            Some(account_name.to_string()),
            Some(realname.clone()),
        ),
    };

    // Standard JOIN format: :nick!user@host JOIN #channel
    let standard_join_msg = Message {
        tags: None,
        prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
        command: Command::JOIN(canonical_name.clone(), None, None),
    };

    // For labeled-response batching: send JOIN to self through ctx.sender
    // (which will be batched with NAMES/topic if label is present)
    // and broadcast to other members
    let self_join_msg = if ctx.handshake.capabilities.contains("extended-join") {
        extended_join_msg.clone()
    } else {
        standard_join_msg.clone()
    };
    ctx.sender.send(self_join_msg).await?;

    // Broadcast to other channel members (exclude self since we sent above)
    ctx.matrix
        .broadcast_to_channel_with_cap(
            &channel_lower,
            extended_join_msg,
            Some(ctx.uid), // Exclude self - we already sent above
            Some("extended-join"),
            Some(standard_join_msg),
        )
        .await;

    // IRCv3 away-notify: Send AWAY to channel members with away-notify cap
    // when a user who is already away joins the channel
    if let Some(ref away_text) = away_message {
        let away_msg = Message {
            tags: None,
            prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
            command: Command::AWAY(Some(away_text.clone())),
        };
        // Only send to others with away-notify, exclude the joining user
        ctx.matrix
            .broadcast_to_channel_with_cap(
                &channel_lower,
                away_msg,
                Some(ctx.uid),
                Some("away-notify"),
                None, // No fallback for clients without away-notify
            )
            .await;
    }

    // If auto-op/voice was applied, broadcast the MODE change using typed modes
    if modes.op || modes.voice {
        let mut mode_changes: Vec<Mode<ChannelMode>> = Vec::new();

        if modes.op {
            mode_changes.push(Mode::plus(ChannelMode::Oper, Some(&nick)));
        }
        if modes.voice {
            mode_changes.push(Mode::plus(ChannelMode::Voice, Some(&nick)));
        }

        let mode_str = mode_changes.iter().map(|m| m.flag()).collect::<String>();

        let mode_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::ChannelMODE(canonical_name.clone(), mode_changes.clone()),
        };

        // Send MODE to self through ctx.sender (for batching with labeled-response)
        ctx.sender.send(mode_msg.clone()).await?;

        // Broadcast to other channel members (exclude self)
        ctx.matrix
            .broadcast_to_channel(&channel_lower, mode_msg, Some(ctx.uid))
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

        // Also send RPL_TOPICWHOTIME (333)
        let topic_who_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_TOPICWHOTIME,
            vec![
                nick.clone(),
                canonical_name.clone(),
                topic.set_by,
                topic.set_at.to_string(),
            ],
        );
        ctx.sender.send(topic_who_reply).await?;
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
    // Channel symbol per RFC 2812: @ = secret, = = public
    let channel_symbol = if channel.modes.secret { "@" } else { "=" };
    drop(channel);

    let names_reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::RPL_NAMREPLY,
        vec![
            nick.clone(),
            channel_symbol.to_string(),
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

    // Check if user is founder - grant op (not owner, since PREFIX=(ov)@+ doesn't include ~)
    if account.id == channel_record.founder_account_id {
        return Some(MemberModes {
            owner: false,
            admin: false,
            op: true,
            halfop: false,
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
        Some(MemberModes {
            owner: false,
            admin: false,
            op,
            halfop: false,
            voice,
            join_time: None, // join_time set by caller
        })
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

    let host = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user_state = user_ref.read().await;
        user_state.host.clone()
    } else if let Some(h) = ctx
        .handshake
        .webirc_host
        .clone()
        .or(ctx.handshake.webirc_ip.clone())
    {
        h
    } else {
        ctx.remote_addr.ip().to_string()
    };

    // Check AKICK list
    ctx.db
        .channels()
        .check_akick(channel_record.id, nick, user, &host)
        .await
        .ok()?
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_>) -> HandlerResult {
    let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
        .await
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
        super::part::leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, None)
            .await?;
    }

    Ok(())
}
