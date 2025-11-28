//! User query handlers: WHO, WHOIS, WHOWAS
//!
//! RFC 2812 §3.6 - User based queries

use super::{server_reply, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Response};
use tracing::debug;

/// Handler for WHO command.
///
/// WHO [<mask> [o]]
/// Returns information about users matching the mask.
/// The 'o' flag restricts results to operators only.
pub struct WhoHandler;

#[async_trait]
impl Handler for WhoHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract mask and operators_only flag
        let (mask, operators_only) = match &msg.command {
            Command::WHO(m, o) => (m.clone(), o.unwrap_or(false)),
            _ => (None, false),
        };

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().unwrap();

        // Determine query type
        if let Some(ref mask_str) = mask {
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
                            let mut flags = "H".to_string(); // TODO: track away status
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
                                    user.host.clone(),
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
                        let mut flags = "H".to_string();
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
                                user.host.clone(),
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
        let end_mask = mask.unwrap_or_else(|| "*".to_string());
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
/// WHOIS [<server>] <nickmask>
/// Returns detailed information about a specific user.
pub struct WhoisHandler;

#[async_trait]
impl Handler for WhoisHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract target nick
        let target = match &msg.command {
            Command::WHOIS(_, target) => target.clone(),
            _ => return Ok(()),
        };

        if target.is_empty() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().unwrap();
        let target_lower = irc_to_lower(&target);

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
                        target_user.host.clone(),
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

                            let prefix = if let Some(member) = channel.members.get(&target_user.uid) {
                                if member.op { "@" } else if member.voice { "+" } else { "" }
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
                send_no_such_nick(ctx, &target).await?;
            }
        } else {
            send_no_such_nick(ctx, &target).await?;
        }

        Ok(())
    }
}

/// Handler for WHOWAS command.
///
/// WHOWAS <nickname> [<count> [<server>]]
/// Returns information about a nickname that no longer exists.
/// Note: Requires whowas history tracking which isn't yet implemented.
pub struct WhowasHandler;

#[async_trait]
impl Handler for WhowasHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract target nick
        let target = match &msg.command {
            Command::WHOWAS(nick, _, _) => nick.clone(),
            _ => String::new(),
        };

        if target.is_empty() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().unwrap();

        // TODO: Implement whowas history tracking
        // For now, always return ERR_WASNOSUCHNICK
        
        // ERR_WASNOSUCHNICK (406): <nick> :There was no such nickname
        let reply = server_reply(
            server_name,
            Response::ERR_WASNOSUCHNICK,
            vec![
                nick.clone(),
                target.clone(),
                "There was no such nickname".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_ENDOFWHOWAS (369): <nick> :End of WHOWAS
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFWHOWAS,
            vec![nick.clone(), target, "End of WHOWAS".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
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
                    Some(vc) if vc == mc => continue,
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
    let nick = ctx.handshake.nick.as_ref().unwrap();

    let reply = server_reply(
        server_name,
        Response::ERR_NOSUCHNICK,
        vec![nick.clone(), target.to_string(), "No such nick/channel".to_string()],
    );
    ctx.sender.send(reply).await?;

    // Also send end of whois
    let reply = server_reply(
        server_name,
        Response::RPL_ENDOFWHOIS,
        vec![nick.clone(), target.to_string(), "End of WHOIS list".to_string()],
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
