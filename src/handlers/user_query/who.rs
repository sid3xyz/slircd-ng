//! WHO handler for listing users matching a mask.
//!
//! Supports both standard WHO (RFC 2812) and WHOX (IRCv3) extensions.

use super::super::{Context, HandlerResult, PostRegHandler, server_reply, with_label};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, MessageRef, Response, irc_to_lower};

/// Handler for WHO command.
///
/// `WHO <mask> [%<fields>[,<token>]]`
///
/// Returns information about users matching the mask.
/// Supports WHOX extensions when %fields are specified.
///
/// **Specification:** [RFC 2812 §3.6.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.6.1)
/// **Extension:** [IRCv3 WHOX](https://ircv3.net/specs/extensions/whox)
pub struct WhoHandler;

/// WHOX field request parsed from %fields string.
#[derive(Default, Clone)]
struct WhoxFields {
    token: bool,          // t
    channel: bool,        // c
    username: bool,       // u
    ip: bool,             // i
    hostname: bool,       // h
    server: bool,         // s
    nick: bool,           // n
    flags: bool,          // f
    hopcount: bool,       // d
    idle: bool,           // l
    account: bool,        // a
    oplevel: bool,        // o
    realname: bool,       // r
    query_token: Option<String>, // The token value if provided
}

impl WhoxFields {
    /// Parse WHOX fields from a string like "%cuhnar" or "%afnt,42"
    fn parse(s: &str) -> Option<Self> {
        if !s.starts_with('%') {
            return None;
        }
        let s = &s[1..]; // Remove %

        // Check for token: %fields,token
        let (fields_str, token_value) = if let Some(comma_pos) = s.find(',') {
            let fields = &s[..comma_pos];
            let token = &s[comma_pos + 1..];
            // Token must be 1-3 digits
            if token.len() > 3 || token.is_empty() || !token.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            (fields, Some(token.to_string()))
        } else {
            (s, None)
        };

        let mut result = WhoxFields {
            query_token: token_value,
            ..Default::default()
        };

        for c in fields_str.chars() {
            match c {
                't' => result.token = true,
                'c' => result.channel = true,
                'u' => result.username = true,
                'i' => result.ip = true,
                'h' => result.hostname = true,
                's' => result.server = true,
                'n' => result.nick = true,
                'f' => result.flags = true,
                'd' => result.hopcount = true,
                'l' => result.idle = true,
                'a' => result.account = true,
                'o' => result.oplevel = true,
                'r' => result.realname = true,
                _ => {} // Ignore unknown fields per spec
            }
        }

        // 't' requires a token value
        if result.token && result.query_token.is_none() {
            return None;
        }

        Some(result)
    }
}

/// User info needed for WHO/WHOX replies.
struct WhoUserInfo<'a> {
    nick: &'a str,
    user: &'a str,
    visible_host: &'a str,
    realname: &'a str,
    account: Option<&'a str>,
    is_away: bool,
    is_oper: bool,
    is_bot: bool,
    channel_prefixes: String,
}

/// Build WHO response (either 352 or 354 depending on WHOX).
fn build_who_reply(
    server_name: &str,
    requester_nick: &str,
    channel: &str,
    user_info: &WhoUserInfo,
    whox: Option<&WhoxFields>,
) -> slirc_proto::Message {
    if let Some(fields) = whox {
        // WHOX: RPL_WHOSPCRPL (354)
        // Order: token, channel, user, ip, host, server, nick, flags, hopcount, idle, account, oplevel, realname
        let mut params = vec![requester_nick.to_string()];

        if fields.token {
            params.push(fields.query_token.clone().unwrap_or_default());
        }
        if fields.channel {
            params.push(channel.to_string());
        }
        if fields.username {
            params.push(user_info.user.to_string());
        }
        if fields.ip {
            // Privacy: don't disclose IP
            params.push("255.255.255.255".to_string());
        }
        if fields.hostname {
            params.push(user_info.visible_host.to_string());
        }
        if fields.server {
            params.push(server_name.to_string());
        }
        if fields.nick {
            params.push(user_info.nick.to_string());
        }
        if fields.flags {
            let mut flags = if user_info.is_away { "G" } else { "H" }.to_string();
            if user_info.is_oper {
                flags.push('*');
            }
            if user_info.is_bot {
                flags.push('B');
            }
            flags.push_str(&user_info.channel_prefixes);
            params.push(flags);
        }
        if fields.hopcount {
            params.push("0".to_string());
        }
        if fields.idle {
            params.push("0".to_string()); // We don't track idle time currently
        }
        if fields.account {
            params.push(user_info.account.unwrap_or("0").to_string());
        }
        if fields.oplevel {
            params.push("n/a".to_string()); // We don't support op levels
        }
        if fields.realname {
            params.push(user_info.realname.to_string());
        }

        server_reply(server_name, Response::RPL_WHOSPCRPL, params)
    } else {
        // Standard WHO: RPL_WHOREPLY (352)
        let mut flags = if user_info.is_away { "G" } else { "H" }.to_string();
        if user_info.is_oper {
            flags.push('*');
        }
        if user_info.is_bot {
            flags.push('B');
        }
        flags.push_str(&user_info.channel_prefixes);

        server_reply(
            server_name,
            Response::RPL_WHOREPLY,
            vec![
                requester_nick.to_string(),
                channel.to_string(),
                user_info.user.to_string(),
                user_info.visible_host.to_string(),
                server_name.to_string(),
                user_info.nick.to_string(),
                flags,
                format!("0 {}", user_info.realname),
            ],
        )
    }
}

/// Build prefix string for WHO flags based on member modes and multi-prefix setting.
fn get_member_prefixes(member_modes: &crate::state::MemberModes, multi_prefix: bool) -> String {
    if multi_prefix {
        member_modes.all_prefix_chars()
    } else if let Some(prefix) = member_modes.prefix_char() {
        prefix.to_string()
    } else {
        String::new()
    }
}

#[async_trait]
impl PostRegHandler for WhoHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let mask = msg.arg(0);
        let second_arg = msg.arg(1);

        // Parse WHOX fields if present, otherwise check for 'o' flag
        let whox = second_arg.and_then(WhoxFields::parse);
        let operators_only = if whox.is_none() {
            second_arg.map(|s| s.eq_ignore_ascii_case("o")).unwrap_or(false)
        } else {
            false // WHOX doesn't use 'o' flag
        };

        let server_name = &ctx.matrix.server_info.name;
        let nick = &ctx.state.nick;

        // Check if the user has multi-prefix CAP enabled
        let user_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.clone());
        let multi_prefix = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.caps.contains("multi-prefix")
        } else {
            false
        };

        // Determine query type
        if let Some(mask_str) = mask {
            if mask_str.is_channel_name() {
                // Channel WHO - list channel members
                let channel_lower = irc_to_lower(mask_str);
                let channel_sender = ctx.matrix.channels.get(&channel_lower).map(|c| c.clone());
                if let Some(channel_sender) = channel_sender {
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

                    // If channel is secret and user is not a member, return nothing (as if channel doesn't exist)
                    if channel_info
                        .modes
                        .contains(&crate::state::actor::ChannelMode::Secret)
                        && !channel_info.is_member
                    {
                        return Ok(());
                    }

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
                        .await;
                    let members = match rx.await {
                        Ok(m) => m,
                        Err(_) => return Ok(()),
                    };

                    // Result count limit to prevent pathologically large responses
                    let max_results = ctx.matrix.config.limits.max_who_results;
                    let mut result_count = 0;
                    let mut truncated = false;

                    for (member_uid, member_modes) in members {
                        // Check limit before emitting each result
                        if result_count >= max_results {
                            truncated = true;
                            break;
                        }

                        let member_arc = ctx.matrix.users.get(&member_uid).map(|u| u.clone());
                        if let Some(member_arc) = member_arc {
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

                            let reply = build_who_reply(
                                server_name,
                                nick,
                                &channel_info.name,
                                &user_info,
                                whox.as_ref(),
                            );
                            ctx.sender.send(reply).await?;
                            result_count += 1;
                        }
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
                }
            } else {
                // Mask-based WHO - search all users
                let mask_lower = irc_to_lower(mask_str);

                // Check if this is an exact nick query (no wildcards)
                let is_exact_query = !mask_str.contains('*') && !mask_str.contains('?');

                // Get requester's operator status for invisible visibility
                let requester_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.clone());
                let requester_is_oper = if let Some(requester_arc) = requester_arc {
                    requester_arc.read().await.modes.oper
                } else {
                    false
                };

                // Pre-collect requester's channel memberships for invisible checking
                let requester_channels: Vec<String> =
                    if let Some(requester_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.clone()) {
                        requester_arc.read().await.channels.iter().cloned().collect()
                    } else {
                        Vec::new()
                    };

                let all_users: Vec<_> = ctx
                    .matrix
                    .users
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect();

                // Result limiting to prevent flooding
                let max_results = ctx.matrix.config.limits.max_who_results;
                let mut result_count = 0;
                let mut truncated = false;

                for (target_uid, user_arc) in all_users {
                    // Check result limit
                    if result_count >= max_results {
                        truncated = true;
                        break;
                    }
                    let user = user_arc.read().await;

                    // Skip if operators_only and not an operator
                    if operators_only && !user.modes.oper {
                        continue;
                    }

                    // Skip invisible users unless:
                    // 1. Requester is an oper
                    // 2. Requester shares a channel with the user
                    // 3. Requester is querying themselves
                    // 4. Query is an exact nick (no wildcards)
                    if user.modes.invisible
                        && !requester_is_oper
                        && target_uid != ctx.uid
                        && !is_exact_query
                    {
                        // Check if they share any channel
                        let mut shares_channel = false;
                        for ch in &user.channels {
                            if requester_channels.contains(ch) {
                                shares_channel = true;
                                break;
                            }
                        }
                        if !shares_channel {
                            continue;
                        }
                    }

                    // Match against nick (simple case-insensitive match or wildcard)
                    let nick_lower = irc_to_lower(&user.nick);
                    if matches_mask(&nick_lower, &mask_lower) {
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

                        let reply = build_who_reply(
                            server_name,
                            nick,
                            "*", // No specific channel
                            &user_info,
                            whox.as_ref(),
                        );
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
            }
        }
        // No mask = return all visible users (typically empty for privacy)

        // RPL_ENDOFWHO (315) - attach label for labeled-response
        let end_mask = mask
            .map(|s| s.to_string())
            .unwrap_or_else(|| "*".to_string());
        let reply = with_label(
            server_reply(
                server_name,
                Response::RPL_ENDOFWHO,
                vec![nick.clone(), end_mask, "End of WHO list".to_string()],
            ),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}


/// Simple wildcard matching for WHO masks.
/// Supports * (match any) and ? (match single char).
fn matches_mask(value: &str, mask: &str) -> bool {
    if mask == "*" {
        return true;
    }
    if !mask.contains('*') && !mask.contains('?') {
        return value == mask;
    }

    // Convert to regex-like matching
    let mut v_chars = value.chars().peekable();
    let mut m_chars = mask.chars().peekable();

    while m_chars.peek().is_some() || v_chars.peek().is_some() {
        match m_chars.peek() {
            Some('*') => {
                m_chars.next();
                if m_chars.peek().is_none() {
                    return true; // Trailing * matches everything
                }
                // Try to match rest of pattern from each position
                let rest_mask: String = m_chars.collect();
                let rest_value: String = v_chars.collect();
                for i in 0..=rest_value.len() {
                    if matches_mask(&rest_value[i..], &rest_mask) {
                        return true;
                    }
                }
                return false;
            }
            Some('?') => {
                m_chars.next();
                if v_chars.next().is_none() {
                    return false;
                }
            }
            Some(mc) => {
                let mc = *mc;
                m_chars.next();
                match v_chars.next() {
                    Some(vc) if vc == mc => {}
                    _ => return false,
                }
            }
            None => return v_chars.peek().is_none(),
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_mask() {
        assert!(matches_mask("test", "test"));
        assert!(matches_mask("test", "*"));
        assert!(matches_mask("test", "t*"));
        assert!(matches_mask("test", "*t"));
        assert!(matches_mask("test", "t*t"));
        assert!(matches_mask("test", "te?t"));
        assert!(matches_mask("test", "????"));
        assert!(!matches_mask("test", "?????"));
        assert!(!matches_mask("test", "best"));
        assert!(matches_mask("testing", "test*"));
        assert!(matches_mask("testing", "*ing"));
    }

    #[test]
    fn test_is_channel_name() {
        use slirc_proto::ChannelExt;
        assert!("#test".is_channel_name());
        assert!("&test".is_channel_name());
        assert!("+test".is_channel_name());
        assert!("!12345test".is_channel_name());
        assert!(!"test".is_channel_name());
        assert!(!"".is_channel_name());
    }
}
