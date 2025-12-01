//! WHOIS handler for detailed user information queries.

use super::super::{Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply, with_label};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tracing::debug;

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

                // RPL_ENDOFWHOIS (318): <nick> :End of WHOIS list - attach label for labeled-response
                let reply = with_label(
                    server_reply(
                        server_name,
                        Response::RPL_ENDOFWHOIS,
                        vec![
                            nick.clone(),
                            target_user.nick.clone(),
                            "End of WHOIS list".to_string(),
                        ],
                    ),
                    ctx.label.as_deref(),
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

    // Also send end of whois - attach label for labeled-response
    let reply = with_label(
        server_reply(
            server_name,
            Response::RPL_ENDOFWHOIS,
            vec![
                nick.clone(),
                target.to_string(),
                "End of WHOIS list".to_string(),
            ],
        ),
        ctx.label.as_deref(),
    );
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Handler for USERHOST command.
///
/// `USERHOST nick [nick ...]`
///
/// Returns the user@host for up to 5 nicknames.
pub struct UserhostHandler;

#[async_trait]
impl Handler for UserhostHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // USERHOST <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "USERHOST".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build response (up to 5 nicks)
        let mut replies = Vec::new();
        for target_nick in nicks.iter().take(5) {
            let target_lower = irc_to_lower(target_nick);
            let uid = ctx.matrix.nicks.get(&target_lower);
            let user_ref = uid.as_ref().and_then(|u| ctx.matrix.users.get(u.value()));
            if let Some(user_ref) = user_ref {
                let user = user_ref.read().await;
                // Format: nick[*]=+/-hostname
                // * if oper, - if away, + if available (RFC 2812)
                let oper_flag = if user.modes.oper { "*" } else { "" };
                let away_flag = if user.away.is_some() { "-" } else { "+" };
                replies.push(format!(
                    "{}{}={}{}@{}",
                    user.nick, oper_flag, away_flag, user.user, user.visible_host
                ));
            }
        }

        // RPL_USERHOST (302)
        let reply = server_reply(
            server_name,
            Response::RPL_USERHOST,
            vec![nick.clone(), replies.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for ISON command.
///
/// `ISON nick [nick ...]`
///
/// Returns which of the given nicknames are online.
pub struct IsonHandler;

#[async_trait]
impl Handler for IsonHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // ISON <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "ISON".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Find which nicks are online
        let mut online = Vec::new();
        for target_nick in nicks {
            let target_lower = irc_to_lower(target_nick);
            if ctx.matrix.nicks.contains_key(&target_lower) {
                // Return the nick as the user typed it (case preserved)
                online.push((*target_nick).to_string());
            }
        }

        // RPL_ISON (303)
        let reply = server_reply(
            server_name,
            Response::RPL_ISON,
            vec![nick.clone(), online.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
