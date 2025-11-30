//! User query handlers: WHO, WHOIS, WHOWAS
//!
//! RFC 2812 §3.6 - User based queries

use super::{Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tracing::debug;

/// Handler for WHO command.
///
/// `WHO [mask [o]]`
///
/// Returns information about users matching the mask.
/// The 'o' flag restricts results to operators only.
pub struct WhoHandler;

#[async_trait]
impl Handler for WhoHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        // WHO [mask] [o]
        let mask = msg.arg(0);
        let operators_only = msg
            .arg(1)
            .map(|s| s.eq_ignore_ascii_case("o"))
            .unwrap_or(false);

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Determine query type
        if let Some(mask_str) = mask {
            if is_channel_name(mask_str) {
                // Channel WHO - list channel members
                let channel_lower = irc_to_lower(mask_str);
                if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
                    let channel = channel_ref.read().await;

                    for (member_uid, member_modes) in &channel.members {
                        if let Some(user_ref) = ctx.matrix.users.get(member_uid) {
                            let user = user_ref.read().await;

                            // Skip if operators_only and not an operator
                            if operators_only && !user.modes.oper {
                                continue;
                            }

                            // Build flags: H=here, G=gone (away), *=ircop, @=chanop, +=voice
                            let mut flags = if user.away.is_some() {
                                "G".to_string()
                            } else {
                                "H".to_string()
                            };
                            if user.modes.oper {
                                flags.push('*');
                            }
                            if member_modes.op {
                                flags.push('@');
                            }
                            if member_modes.voice && !member_modes.op {
                                flags.push('+');
                            }

                            // RPL_WHOREPLY (352): <channel> <user> <host> <server> <nick> <flags> :<hopcount> <realname>
                            let reply = server_reply(
                                server_name,
                                Response::RPL_WHOREPLY,
                                vec![
                                    nick.clone(),
                                    channel.name.clone(),
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

                for user_ref in ctx.matrix.users.iter() {
                    let user = user_ref.read().await;

                    // Skip if operators_only and not an operator
                    if operators_only && !user.modes.oper {
                        continue;
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

        // RPL_ENDOFWHO (315)
        let end_mask = mask
            .map(|s| s.to_string())
            .unwrap_or_else(|| "*".to_string());
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFWHO,
            vec![nick.clone(), end_mask, "End of WHO list".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for WHOIS command.
///
/// `WHOIS [server] nickmask`
///
/// Returns detailed information about a specific user.
pub struct WhoisHandler;

#[async_trait]
impl Handler for WhoisHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        // WHOIS [server] <nick>
        // If two args, first is server, second is nick
        // If one arg, it's the nick
        let target = if msg.args().len() >= 2 {
            msg.arg(1).unwrap_or("")
        } else {
            msg.arg(0).unwrap_or("")
        };

        if target.is_empty() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let target_lower = irc_to_lower(target);

        // Look up target user
        if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
            if let Some(target_user_ref) = ctx.matrix.users.get(target_uid.value()) {
                let target_user = target_user_ref.read().await;

                // RPL_WHOISUSER (311): <nick> <user> <host> * :<realname>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOISUSER,
                    vec![
                        nick.clone(),
                        target_user.nick.clone(),
                        target_user.user.clone(),
                        target_user.visible_host.clone(),
                        "*".to_string(),
                        target_user.realname.clone(),
                    ],
                );
                ctx.sender.send(reply).await?;

                // RPL_WHOISSERVER (312): <nick> <server> :<server info>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOISSERVER,
                    vec![
                        nick.clone(),
                        target_user.nick.clone(),
                        server_name.clone(),
                        ctx.matrix.server_info.description.clone(),
                    ],
                );
                ctx.sender.send(reply).await?;

                // RPL_WHOISCHANNELS (319): <nick> :{[@|+]<channel>}
                if !target_user.channels.is_empty() {
                    let mut channel_list = Vec::new();
                    for channel_name in &target_user.channels {
                        if let Some(channel_ref) = ctx.matrix.channels.get(channel_name) {
                            let channel = channel_ref.read().await;

                            // Skip secret channels unless requester is a member
                            if channel.modes.secret && !channel.is_member(ctx.uid) {
                                continue;
                            }

                            let prefix = if let Some(member) = channel.members.get(&target_user.uid)
                            {
                                if member.op {
                                    "@"
                                } else if member.voice {
                                    "+"
                                } else {
                                    ""
                                }
                            } else {
                                ""
                            };
                            channel_list.push(format!("{}{}", prefix, channel.name));
                        }
                    }

                    if !channel_list.is_empty() {
                        let reply = server_reply(
                            server_name,
                            Response::RPL_WHOISCHANNELS,
                            vec![
                                nick.clone(),
                                target_user.nick.clone(),
                                channel_list.join(" "),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
                }

                // RPL_WHOISOPERATOR (313): <nick> :is an IRC operator
                if target_user.modes.oper {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISOPERATOR,
                        vec![
                            nick.clone(),
                            target_user.nick.clone(),
                            "is an IRC operator".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_WHOISSECURE (671): <nick> :is using a secure connection (if TLS)
                if target_user.modes.secure {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISSECURE,
                        vec![
                            nick.clone(),
                            target_user.nick.clone(),
                            "is using a secure connection".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_AWAY (301): <nick> :<away message>
                if let Some(away_msg) = &target_user.away {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_AWAY,
                        vec![nick.clone(), target_user.nick.clone(), away_msg.clone()],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_ENDOFWHOIS (318): <nick> :End of WHOIS list
                let reply = server_reply(
                    server_name,
                    Response::RPL_ENDOFWHOIS,
                    vec![
                        nick.clone(),
                        target_user.nick.clone(),
                        "End of WHOIS list".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;

                debug!(requester = %nick, target = %target_user.nick, "WHOIS completed");
            } else {
                send_no_such_nick(ctx, target).await?;
            }
        } else {
            send_no_such_nick(ctx, target).await?;
        }

        Ok(())
    }
}

/// Handler for WHOWAS command.
///
/// `WHOWAS nickname [count [server]]`
///
/// Returns information about a nickname that no longer exists.
/// Queries the WHOWAS history stored in Matrix.
pub struct WhowasHandler;

#[async_trait]
impl Handler for WhowasHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        // WHOWAS <nick> [count [server]]
        let target = msg.arg(0).unwrap_or("");
        let count: usize = msg
            .arg(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(10) // Default to 10 entries
            .min(10); // Cap at 10

        if target.is_empty() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Look up WHOWAS history
        let target_lower = irc_to_lower(target);

        if let Some(entries) = ctx.matrix.whowas.get(&target_lower) {
            let entries_to_show: Vec<_> = entries.iter().take(count).cloned().collect();

            if entries_to_show.is_empty() {
                // No entries found
                let reply = server_reply(
                    server_name,
                    Response::ERR_WASNOSUCHNICK,
                    vec![
                        nick.clone(),
                        target.to_string(),
                        "There was no such nickname".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            } else {
                // Send RPL_WHOWASUSER for each entry
                for entry in entries_to_show {
                    // RPL_WHOWASUSER (314): <nick> <user> <host> * :<realname>
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOWASUSER,
                        vec![
                            nick.clone(),
                            entry.nick,
                            entry.user,
                            entry.host,
                            "*".to_string(),
                            entry.realname,
                        ],
                    );
                    ctx.sender.send(reply).await?;

                    // RPL_WHOISSERVER (312): <nick> <server> :<server info>
                    // Note: Using same numeric for server info in WHOWAS
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISSERVER,
                        vec![
                            nick.clone(),
                            target.to_string(),
                            entry.server.clone(),
                            format!("Logged out at {}", format_timestamp(entry.logout_time)),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }
            }
        } else {
            // No history for this nick at all
            let reply = server_reply(
                server_name,
                Response::ERR_WASNOSUCHNICK,
                vec![
                    nick.clone(),
                    target.to_string(),
                    "There was no such nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_ENDOFWHOWAS (369): <nick> :End of WHOWAS
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFWHOWAS,
            vec![
                nick.clone(),
                target.to_string(),
                "End of WHOWAS".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Format a Unix timestamp as a human-readable string.
fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Check if a string is a valid channel name.
fn is_channel_name(name: &str) -> bool {
    matches!(name.chars().next(), Some('#' | '&' | '+' | '!'))
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

/// Send ERR_NOSUCHNICK for a target.
async fn send_no_such_nick(ctx: &mut Context<'_>, target: &str) -> HandlerResult {
    let server_name = &ctx.matrix.server_info.name;
    let nick = ctx
        .handshake
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;

    let reply = server_reply(
        server_name,
        Response::ERR_NOSUCHNICK,
        vec![
            nick.clone(),
            target.to_string(),
            "No such nick/channel".to_string(),
        ],
    );
    ctx.sender.send(reply).await?;

    // Also send end of whois
    let reply = server_reply(
        server_name,
        Response::RPL_ENDOFWHOIS,
        vec![
            nick.clone(),
            target.to_string(),
            "End of WHOIS list".to_string(),
        ],
    );
    ctx.sender.send(reply).await?;

    Ok(())
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
        assert!(is_channel_name("#test"));
        assert!(is_channel_name("&test"));
        assert!(is_channel_name("+test"));
        assert!(is_channel_name("!12345test"));
        assert!(!is_channel_name("test"));
        assert!(!is_channel_name(""));
    }
}
