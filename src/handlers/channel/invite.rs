//! INVITE command handler
//!
//! RFC 2812 - Channel invitation
//!
//! Uses CapabilityAuthority (Innovation 4) for centralized authorization.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, server_notice, server_reply,
    user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// Rate limit cooldown for INVITE command (per target user)
const INVITE_COOLDOWN: Duration = Duration::from_secs(30);

/// Handler for INVITE command.
///
/// `INVITE nickname channel`
///
/// Invites a user to a channel.
pub struct InviteHandler;

#[async_trait]
impl PostRegHandler for InviteHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // Own server_name to avoid borrowing ctx for the entire function
        let server_name = ctx.server_name().to_string();
        let nick = ctx.state.nick.clone();

        // INVITE <nickname> <channel> or INVITE <channel> <nickname>
        // Detect which argument is which based on whether it starts with a channel prefix
        let arg0 = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let arg1 = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        // Track if channel was first (non-standard order) for echo
        let channel_first = arg0.starts_with('#')
            || arg0.starts_with('&')
            || arg0.starts_with('+')
            || arg0.starts_with('!');

        let (target_nick, channel_name) = if channel_first {
            // INVITE #channel nickname format
            (arg1, arg0)
        } else {
            // INVITE nickname #channel format (standard)
            (arg0, arg1)
        };

        let channel_lower = irc_to_lower(channel_name);
        let target_lower = irc_to_lower(target_nick);

        // Rate limit INVITE: 30-second cooldown per target:channel combination
        // This prevents spam invitations to the same user for the same channel
        let invite_key = format!("{}:{}", target_lower, channel_lower);
        let now = Instant::now();

        // Check rate limit
        if let Some(&last_time) = ctx.state.invite_timestamps.get(&invite_key)
            && now.duration_since(last_time) < INVITE_COOLDOWN
        {
            let secs_left = (INVITE_COOLDOWN - now.duration_since(last_time)).as_secs();
            let reply = server_notice(
                &server_name,
                &nick,
                format!(
                    "Cannot invite {} to {} (rate limited). Try again in {} seconds.",
                    target_nick, channel_name, secs_left
                ),
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Check if target exists
        let target_uid = match ctx.matrix.user_manager.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply =
                    Response::err_nosuchnick(&nick, target_nick).with_prefix(ctx.server_prefix());
                ctx.send_error("INVITE", "ERR_NOSUCHNICK", reply).await?;
                return Ok(());
            }
        };

        // Check if channel exists
        let channel_tx = ctx
            .matrix
            .channel_manager
            .channels
            .get(&channel_lower)
            .map(|c| c.clone());
        if let Some(channel_tx) = channel_tx {
            // Check if user is on channel
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.clone());
            let user_in_channel = if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                user.channels.contains(&channel_lower)
            } else {
                false
            };

            if !user_in_channel {
                let reply = Response::err_notonchannel(&nick, channel_name)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("INVITE", "ERR_NOTONCHANNEL", reply).await?;
                return Ok(());
            }

            let (reply_tx, reply_rx) = oneshot::channel();
            let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;
            let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

            // Request INVITE capability from authority (Innovation 4)
            let authority = ctx.authority();
            let invite_cap = authority.request_invite_cap(ctx.uid, channel_name).await;

            let event = ChannelEvent::Invite {
                params: crate::state::actor::InviteParams {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix: sender_prefix.clone(),
                    target_uid: target_uid.clone(),
                    target_nick: target_nick.to_string(),
                    force: false, // Deprecated in favor of cap
                    cap: invite_cap,
                },
                reply_tx,
            };

            if (channel_tx.send(event).await).is_err() {
                return Ok(());
            }

            match reply_rx.await {
                Ok(Ok(())) => {
                    // Success, invite recorded in channel.

                    // Record rate limit for this successful invite
                    ctx.state.invite_timestamps.insert(invite_key.clone(), now);
                    ctx.state
                        .invite_timestamps
                        .retain(|_, t| now.duration_since(*t) < INVITE_COOLDOWN);
                    // Limit map size to prevent memory exhaustion
                    if ctx.state.invite_timestamps.len() > 50 {
                        let oldest = ctx
                            .state
                            .invite_timestamps
                            .iter()
                            .min_by_key(|(_, t)| *t)
                            .map(|(k, _)| k.clone());
                        if let Some(oldest_key) = oldest {
                            ctx.state.invite_timestamps.remove(&oldest_key);
                        }
                    }

                    // Now send INVITE message to target user.

                    // Get sender's account for account-tag
                    let sender_arc = ctx
                        .matrix
                        .user_manager
                        .users
                        .get(ctx.uid)
                        .map(|u| u.clone());
                    let sender_account: Option<String> = if let Some(sender_arc) = sender_arc {
                        let sender_user = sender_arc.read().await;
                        sender_user.account.clone()
                    } else {
                        None
                    };

                    // Build invite message with appropriate tags
                    let mut invite_tags: Option<Vec<slirc_proto::message::Tag>> = None;

                    // Check if target has account-tag capability
                    let target_arc = ctx
                        .matrix
                        .user_manager
                        .users
                        .get(&target_uid)
                        .map(|u| u.clone());
                    if let (Some(account), Some(target_arc)) = (sender_account.as_ref(), target_arc)
                    {
                        let target_user = target_arc.read().await;
                        if target_user.caps.contains("account-tag") {
                            invite_tags = Some(vec![slirc_proto::message::Tag(
                                std::borrow::Cow::Borrowed("account"),
                                Some(account.clone()),
                            )]);
                        }
                    }

                    let invite_msg = Message {
                        tags: invite_tags,
                        prefix: Some(sender_prefix),
                        command: if channel_first {
                            Command::INVITE(channel_name.to_string(), target_nick.to_string())
                        } else {
                            Command::INVITE(target_nick.to_string(), channel_name.to_string())
                        },
                    };

                    let target_sender = ctx
                        .matrix
                        .user_manager
                        .senders
                        .get(&target_uid)
                        .map(|s| s.clone());
                    if let Some(target_sender) = target_sender {
                        let _ = target_sender.send(invite_msg).await;
                    }

                    // RPL_INVITING (341)
                    let reply = server_reply(
                        &server_name,
                        Response::RPL_INVITING,
                        vec![
                            nick.clone(),
                            target_nick.to_string(),
                            channel_name.to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }
                Ok(Err(e)) => {
                    let reply = e.to_irc_reply(&server_name, &nick, channel_name);
                    ctx.sender.send(reply).await?;
                }
                Err(_) => {}
            }
        } else {
            // Channel doesn't exist - RFC1459/2812 allows invites to non-existent channels
            // "There is no requirement that the channel the target user is being
            // invited to must exist or be a valid channel."

            let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
                .await
                .ok_or(HandlerError::NickOrUserMissing)?;
            let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

            // Get sender's account for account-tag
            let sender_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.clone());
            let sender_account: Option<String> = if let Some(sender_arc) = sender_arc {
                let sender_user = sender_arc.read().await;
                sender_user.account.clone()
            } else {
                None
            };

            // Build invite tags with account if target has capability
            let mut invite_tags: Option<Vec<slirc_proto::message::Tag>> = None;
            let target_arc = ctx
                .matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            if let (Some(account), Some(target_arc)) = (sender_account.as_ref(), target_arc) {
                let target_user = target_arc.read().await;
                if target_user.caps.contains("account-tag") {
                    invite_tags = Some(vec![slirc_proto::message::Tag(
                        std::borrow::Cow::Borrowed("account"),
                        Some(account.clone()),
                    )]);
                }
            }

            // Send INVITE notification to target
            let invite_msg = Message {
                tags: invite_tags,
                prefix: Some(sender_prefix.clone()),
                command: if channel_first {
                    Command::INVITE(channel_name.to_string(), target_nick.to_string())
                } else {
                    Command::INVITE(target_nick.to_string(), channel_name.to_string())
                },
            };

            let target_sender = ctx
                .matrix
                .user_manager
                .senders
                .get(&target_uid)
                .map(|s| s.clone());
            if let Some(target_sender) = target_sender {
                let _ = target_sender.send(invite_msg).await;
            }

            // Echo INVITE back to sender
            let echo_msg = Message {
                tags: None,
                prefix: Some(sender_prefix),
                command: if channel_first {
                    Command::INVITE(channel_name.to_string(), target_nick.to_string())
                } else {
                    Command::INVITE(target_nick.to_string(), channel_name.to_string())
                },
            };
            ctx.sender.send(echo_msg).await?;
        }

        Ok(())
    }
}
