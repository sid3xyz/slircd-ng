//! WHOIS handler for detailed user information queries.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply, with_label};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, ChannelInfo};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tokio::sync::mpsc::Sender;
use tracing::debug;

/// Get channel info and member modes for a user in a channel.
///
/// Returns None if the channel should be hidden (secret and requester not member).
async fn get_channel_display_info(
    channel_sender: &Sender<ChannelEvent>,
    requester_uid: &str,
    target_uid: &str,
    channel_name: &str,
) -> Option<String> {
    // Get channel info
    let (tx, rx) = tokio::sync::oneshot::channel();
    channel_sender
        .send(ChannelEvent::GetInfo {
            requester_uid: Some(requester_uid.to_string()),
            reply_tx: tx,
        })
        .await
        .ok()?;

    let info: ChannelInfo = rx.await.ok()?;

    // Skip secret channels unless requester is a member
    if info.modes.contains(&crate::state::actor::ChannelMode::Secret) && !info.is_member {
        return None;
    }

    // Get member modes for prefix
    let (tx, rx) = tokio::sync::oneshot::channel();
    channel_sender
        .send(ChannelEvent::GetMemberModes {
            uid: target_uid.to_string(),
            reply_tx: tx,
        })
        .await
        .ok()?;

    let prefix = match rx.await {
        Ok(Some(modes)) if modes.op => "@",
        Ok(Some(modes)) if modes.voice => "+",
        _ => "",
    };

    Some(format!("{}{}", prefix, channel_name))
}

/// Handler for WHOIS command.
///
/// `WHOIS [server] nickmask`
///
/// Returns detailed information about a specific user.
pub struct WhoisHandler;

#[async_trait]
impl PostRegHandler for WhoisHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

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
                ctx.server_name(),
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.state.nick.clone(),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick; // Guaranteed present in RegisteredState
        let target_lower = irc_to_lower(target);

        // Look up target user
        let target_uid = ctx.matrix.nicks.get(&target_lower).map(|r| r.value().clone());
        if let Some(target_uid) = target_uid {
            let target_user_arc = ctx.matrix.users.get(&target_uid).map(|u| u.clone());
            if let Some(target_user_arc) = target_user_arc {
                // Clone needed data, drop lock immediately to prevent holding during async ops
                let (
                    target_nick,
                    target_user_name,
                    target_visible_host,
                    target_realname,
                    target_channels,
                    target_modes,
                    target_account,
                    target_away,
                    target_uid_owned,
                ) = {
                    let target_user = target_user_arc.read().await;
                    (
                        target_user.nick.clone(),
                        target_user.user.clone(),
                        target_user.visible_host.clone(),
                        target_user.realname.clone(),
                        target_user.channels.clone(),
                        target_user.modes.clone(),
                        target_user.account.clone(),
                        target_user.away.clone(),
                        target_user.uid.clone(),
                    )
                }; // Lock dropped here

                // RPL_WHOISUSER (311): <nick> <user> <host> * :<realname>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOISUSER,
                    vec![
                        nick.clone(),
                        target_nick.clone(),
                        target_user_name,
                        target_visible_host,
                        "*".to_string(),
                        target_realname,
                    ],
                );
                ctx.sender.send(reply).await?;

                // RPL_WHOISSERVER (312): <nick> <server> :<server info>
                let reply = server_reply(
                    server_name,
                    Response::RPL_WHOISSERVER,
                    vec![
                        nick.clone(),
                        target_nick.clone(),
                        server_name.to_string(),
                        ctx.matrix.server_info.description.clone(),
                    ],
                );
                ctx.sender.send(reply).await?;

                // RPL_WHOISCHANNELS (319): <nick> :{[@|+]<channel>}
                // Skip if target is invisible and requester doesn't share any channels
                let show_channels = if target_modes.invisible && target_uid != ctx.uid {
                    // Check if requester shares any channel with target
                    let mut shares_channel = false;
                    let requester_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.clone());
                    if let Some(requester_arc) = requester_arc {
                        let requester = requester_arc.read().await;
                        for ch in &target_channels {
                            if requester.channels.contains(ch) {
                                shares_channel = true;
                                break;
                            }
                        }
                    }
                    shares_channel
                } else {
                    true
                };

                if show_channels && !target_channels.is_empty() {
                    let mut channel_list = Vec::new();
                    for channel_name in &target_channels {
                        let Some(channel_sender) = ctx.matrix.channels.get(channel_name) else {
                            continue;
                        };
                        if let Some(display) = get_channel_display_info(
                            channel_sender.value(),
                            ctx.uid,
                            &target_uid_owned,
                            channel_name,
                        )
                        .await
                        {
                            channel_list.push(display);
                        }
                    }

                    if !channel_list.is_empty() {
                        let reply = server_reply(
                            server_name,
                            Response::RPL_WHOISCHANNELS,
                            vec![nick.clone(), target_nick.clone(), channel_list.join(" ")],
                        );
                        ctx.sender.send(reply).await?;
                    }
                }

                // RPL_WHOISOPERATOR (313): <nick> :is an IRC operator
                if target_modes.oper {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISOPERATOR,
                        vec![
                            nick.clone(),
                            target_nick.clone(),
                            "is an IRC operator".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_WHOISBOT (335): <nick> :is a Bot
                if target_modes.bot {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISBOT,
                        vec![
                            nick.clone(),
                            target_nick.clone(),
                            format!("is a Bot on {}", ctx.matrix.server_info.network),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_WHOISACCOUNT (330): <nick> <account> :is logged in as
                if let Some(account) = &target_account {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISACCOUNT,
                        vec![
                            nick.clone(),
                            target_nick.clone(),
                            account.clone(),
                            "is logged in as".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_WHOISSECURE (671): <nick> :is using a secure connection (if TLS)
                if target_modes.secure {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISSECURE,
                        vec![
                            nick.clone(),
                            target_nick.clone(),
                            "is using a secure connection".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }

                // RPL_AWAY (301): <nick> :<away message>
                if let Some(away_msg) = &target_away {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_AWAY,
                        vec![nick.clone(), target_nick.clone(), away_msg.clone()],
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
                            target_nick.clone(),
                            "End of WHOIS list".to_string(),
                        ],
                    ),
                    ctx.label.as_deref(),
                );
                ctx.sender.send(reply).await?;

                debug!(requester = %nick, target = %target_nick, "WHOIS completed");
            } else {
                send_no_such_nick(ctx, target).await?;
            }
        } else {
            send_no_such_nick(ctx, target).await?;
        }

        Ok(())
    }
}

/// Send ERR_NOSUCHNICK for a target, followed by RPL_ENDOFWHOIS.
async fn send_no_such_nick(ctx: &mut Context<'_, crate::state::RegisteredState>, target: &str) -> HandlerResult {
    let server_name = ctx.server_name();
    let nick = &ctx.state.nick;

    let reply = Response::err_nosuchnick(nick, target)
        .with_prefix(ctx.server_prefix());
    ctx.send_error("WHOIS", "ERR_NOSUCHNICK", reply).await?;

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
