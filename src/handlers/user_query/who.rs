//! WHO handler for listing users matching a mask.

use super::super::{Context, HandlerResult, PostRegHandler, server_reply, with_label};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, MessageRef, Response, irc_to_lower};

/// Handler for WHO command.
///
/// `WHO [mask [o]]`
///
/// Returns information about users matching the mask.
/// The 'o' flag restricts results to operators only.
pub struct WhoHandler;

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
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // WHO [mask] [o]
        let mask = msg.arg(0);
        let operators_only = msg
            .arg(1)
            .map(|s| s.eq_ignore_ascii_case("o"))
            .unwrap_or(false);

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

                    for (member_uid, member_modes) in members {
                        let member_arc = ctx.matrix.users.get(&member_uid).map(|u| u.clone());
                        if let Some(member_arc) = member_arc {
                            let user = member_arc.read().await;

                            // Skip if operators_only and not an operator
                            if operators_only && !user.modes.oper {
                                continue;
                            }

                            // Build flags: H=here, G=gone (away), *=ircop, then channel prefixes
                            let mut flags = if user.away.is_some() {
                                "G".to_string()
                            } else {
                                "H".to_string()
                            };
                            if user.modes.oper {
                                flags.push('*');
                            }
                            flags.push_str(&get_member_prefixes(&member_modes, multi_prefix));

                            // RPL_WHOREPLY (352): <channel> <user> <host> <server> <nick> <flags> :<hopcount> <realname>
                            let reply = server_reply(
                                server_name,
                                Response::RPL_WHOREPLY,
                                vec![
                                    nick.clone(),
                                    channel_info.name.clone(),
                                    user.user.clone(),
                                    user.visible_host.clone(),
                                    server_name.clone(),
                                    user.nick.clone(),
                                    flags,
                                    format!("0 {}", user.realname),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                        }
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

                for (target_uid, user_arc) in all_users {
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
                        let mut flags = if user.away.is_some() {
                            "G".to_string()
                        } else {
                            "H".to_string()
                        };
                        if user.modes.oper {
                            flags.push('*');
                        }

                        let reply = server_reply(
                            server_name,
                            Response::RPL_WHOREPLY,
                            vec![
                                nick.clone(),
                                "*".to_string(), // No specific channel
                                user.user.clone(),
                                user.visible_host.clone(),
                                server_name.clone(),
                                user.nick.clone(),
                                flags,
                                format!("0 {}", user.realname),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
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
