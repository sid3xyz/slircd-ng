use super::common::{get_member_prefixes, matches_mask, WhoUserInfo};
use crate::handlers::{Context, HandlerResult, server_reply};
use crate::state::RegisteredState;
use slirc_proto::{irc_to_lower, Response, Message};

/// Execute WHO search on a channel.
pub async fn search_channel_users<F>(
    ctx: &mut Context<'_, RegisteredState>,
    channel_name: &str,
    operators_only: bool,
    multi_prefix: bool,
    mut callback: F,
) -> HandlerResult
where
    F: FnMut(WhoUserInfo, &str) -> Message,
{
    let server_name = ctx.server_name();
    let nick = &ctx.state.nick;
    let channel_lower = irc_to_lower(channel_name);

    let channel_sender = match ctx.matrix.channel_manager.channels.get(&channel_lower) {
        Some(c) => c.clone(),
        None => return Ok(()),
    };

    // Get channel info
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = channel_sender
        .send(crate::state::actor::ChannelEvent::GetInfo {
            requester_uid: Some(ctx.uid.to_string()),
            reply_tx: tx,
        })
        .await;

    let channel_info = match rx.await {
        Ok(info) => info,
        Err(_) => return Ok(()),
    };

    // If channel is secret and user is not a member, return nothing
    if channel_info
        .modes
        .contains(&crate::state::actor::ChannelMode::Secret)
        && !channel_info.is_member
    {
        return Ok(());
    }

    // Get members
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = channel_sender
        .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
        .await;
    let members = match rx.await {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    // Result count limit
    let max_results = ctx.matrix.config.limits.max_who_results;
    let mut result_count = 0;
    let mut truncated = false;

    for (member_uid, member_modes) in members {
        if result_count >= max_results {
            truncated = true;
            break;
        }

        let member_arc = match ctx.matrix.user_manager.users.get(&member_uid) {
            Some(u) => u.clone(),
            None => continue,
        };
        let user = member_arc.read().await;

        // Skip if operators_only and not an operator
        if operators_only && !user.modes.oper {
            continue;
        }

        let user_info = WhoUserInfo {
            nick: &user.nick,
            user: &user.user,
            visible_host: &user.visible_host,
            realname: &user.realname,
            account: user.account.as_deref(),
            is_away: user.away.is_some(),
            is_oper: user.modes.oper,
            is_bot: user.modes.bot,
            channel_prefixes: get_member_prefixes(&member_modes, multi_prefix),
        };

        let reply = callback(user_info, &channel_info.name);
        ctx.sender.send(reply).await?;
        result_count += 1;
    }

    // Notify if output was truncated
    if truncated {
        let truncate_notice = server_reply(
            server_name,
            Response::RPL_TRYAGAIN,
            vec![
                nick.clone(),
                "WHO".to_string(),
                format!("Output truncated, {} matches shown", max_results),
            ],
        );
        ctx.sender.send(truncate_notice).await?;
    }

    Ok(())
}

/// Execute WHO search on a mask (nick/host/realname/server).
pub async fn search_mask_users<F>(
    ctx: &mut Context<'_, RegisteredState>,
    mask_str: &str,
    operators_only: bool,
    _multi_prefix: bool, // Unused for mask WHO
    mut callback: F,
) -> HandlerResult
where
    F: FnMut(WhoUserInfo, &str) -> Message,
{
    let server_name = ctx.server_name();
    let nick = &ctx.state.nick;
    let mask_lower = irc_to_lower(mask_str);

    // Check if this is an exact nick query (no wildcards)
    let is_exact_query = !mask_str.contains('*') && !mask_str.contains('?');

    // Get requester's operator status for invisible visibility
    let requester_is_oper = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.clone())
        .map(|arc| {
            // We need to check synchronously - use try_read
            arc.try_read().map(|u| u.modes.oper).unwrap_or(false)
        })
        .unwrap_or(false);

    // Pre-collect requester's channel memberships for invisible checking
    let requester_channels: Vec<String> = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.clone())
        .map(|arc| {
            arc.try_read()
                .map(|u| u.channels.iter().cloned().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    // Collect all users
    let all_users: Vec<_> = ctx
        .matrix
        .user_manager
        .users
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();

    // Result limiting
    let max_results = ctx.matrix.config.limits.max_who_results;
    let mut result_count = 0;
    let mut truncated = false;

    for (target_uid, user_arc) in all_users {
        if result_count >= max_results {
            truncated = true;
            break;
        }
        let user = user_arc.read().await;

        // Skip service users (+S) - they should not appear in WHO results
        if user.modes.service {
            continue;
        }

        // Skip if operators_only and not an operator
        if operators_only && !user.modes.oper {
            continue;
        }

        // Check invisible user visibility
        if user.modes.invisible && !requester_is_oper && target_uid != ctx.uid && !is_exact_query {
            // Check if they share any channel
            let shares_channel = user
                .channels
                .iter()
                .any(|ch| requester_channels.contains(ch));
            if !shares_channel {
                continue;
            }
        }

        // Match against nick, username, visible_host, or realname (RFC 2812)
        let nick_lower = irc_to_lower(&user.nick);
        let user_lower = irc_to_lower(&user.user);
        let host_lower = irc_to_lower(&user.visible_host);
        let realname_lower = irc_to_lower(&user.realname);

        let matches = matches_mask(&nick_lower, &mask_lower)
            || matches_mask(&user_lower, &mask_lower)
            || matches_mask(&host_lower, &mask_lower)
            || matches_mask(&realname_lower, &mask_lower);

        if matches {
            let user_info = WhoUserInfo {
                nick: &user.nick,
                user: &user.user,
                visible_host: &user.visible_host,
                realname: &user.realname,
                account: user.account.as_deref(),
                is_away: user.away.is_some(),
                is_oper: user.modes.oper,
                is_bot: user.modes.bot,
                channel_prefixes: String::new(), // No channel context for mask WHO
            };

            let reply = callback(user_info, "*");
            ctx.sender.send(reply).await?;
            result_count += 1;
        }
    }

    // Notify if results were truncated
    if truncated {
        let notice = server_reply(
            server_name,
            Response::RPL_TRYAGAIN,
            vec![
                nick.clone(),
                "WHO".to_string(),
                format!("Output truncated, {} results max", max_results),
            ],
        );
        ctx.sender.send(notice).await?;
    }

    Ok(())
}
