//! RELAYMSG command handler (Ergo extension).
//!
//! The RELAYMSG command allows relaying messages between IRC networks.
//! This is an Ergo extension for network bridges and bouncers.
//!
//! Format: `RELAYMSG <target> <relay_from> :<text>`
//!
//! Where:
//! - target: The destination (channel or user)
//! - relay_from: The advertised sender (network/server/nick format)
//! - text: The message content
//!
//! The relayed message appears with a special prefix indicating the relay source.
//! Only IRC operators can use this command (security measure).

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, server_reply};
use super::common::{
    RouteMeta, RouteOptions, SenderSnapshot, route_to_channel_with_snapshot,
    route_to_user_with_snapshot,
};
use crate::handlers::helpers::with_label;
use crate::history::{MessageEnvelope, StoredMessage};
use crate::state::RegisteredState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::debug;

pub struct RelayMsgHandler;

#[async_trait]
impl PostRegHandler for RelayMsgHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Extract RELAYMSG parameters
        // Proto now correctly parses: RELAYMSG <target> <relay_from> <text>
        let relay_from = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        if relay_from.is_empty() || target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Validate relay_from nick format FIRST (before oper check)
        // Valid format: "nick/service" (e.g., "smt/discord")
        // Invalid: contains '!' or missing '/' designator
        if relay_from.contains('!') || !relay_from.contains('/') {
            let reply = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
                command: Command::FAIL(
                    "RELAYMSG".to_string(),
                    "INVALID_NICK".to_string(),
                    vec![format!("Invalid relay nick format: {}", relay_from)],
                ),
            };
            let reply = with_label(reply, ctx.label.as_deref());
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build sender snapshot for routing
        let snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RELAYMSG requires channel operator privileges on the target channel.
        // Check membership and chanop status before routing.
        if target.is_channel_name() {
            let target_lower = irc_to_lower(target);

            // Query channel actor for member modes using oneshot channel pattern
            let has_privs =
                if let Some(channel_tx) = ctx.matrix.channel_manager.channels.get(&target_lower) {
                    let (reply_tx, reply_rx) = oneshot::channel();
                    let event = ChannelEvent::GetMemberModes {
                        uid: ctx.uid.to_string(),
                        reply_tx,
                    };
                    if channel_tx.send(event).await.is_ok() {
                        match reply_rx.await {
                            Ok(Some(modes)) => modes.has_op_or_higher(),
                            _ => false,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

            if !has_privs {
                let reply = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
                    command: Command::FAIL(
                        "RELAYMSG".to_string(),
                        "PRIVS_NEEDED".to_string(),
                        vec!["You do not have channel privileges to use RELAYMSG".to_string()],
                    ),
                };
                let reply = with_label(reply, ctx.label.as_deref());
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Create the relay prefix (relay_from!user@host).
        // Per proto fix, the visible host is the actual connecting IP.
        let relay_prefix = Prefix::Nickname(
            relay_from.to_string(),
            snapshot.user.clone(),
            snapshot.ip.clone(),
        );

        let relayed_msg = Message {
            tags: None,
            prefix: Some(relay_prefix.clone()),
            command: slirc_proto::Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        // echo-message: echo labeled PRIVMSG back to sender BEFORE routing.
        // This satisfies both echo-message and labeled-response specs.
        let echo_msg = with_label(relayed_msg.clone(), ctx.label.as_deref());
        ctx.sender.send(echo_msg).await?;

        // Determine if target is a channel or user
        if target.is_channel_name() {
            // Channel target
            let target_lower = irc_to_lower(target);

            // Check if channel exists
            if !ctx
                .matrix
                .channel_manager
                .channels
                .contains_key(&target_lower)
            {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.state.nick.clone(),
                        target.to_string(),
                        "No such channel".to_string(),
                    ],
                );
                let reply = with_label(reply, ctx.label.as_deref());
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            let route_opts = RouteOptions {
                send_away_reply: false,
                status_prefix: None,
            };

            // Generate msgid and timestamp for history
            let msgid = uuid::Uuid::new_v4().to_string();
            let nanotime = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
            let timestamp_iso = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();

            let _ = route_to_channel_with_snapshot(
                ctx,
                &target_lower,
                relayed_msg,
                &route_opts,
                RouteMeta {
                    timestamp: Some(timestamp_iso.clone()),
                    msgid: Some(msgid.clone()),
                    override_nick: Some(relay_from.to_string()),
                    relaymsg_sender_nick: Some(snapshot.nick.clone()),
                },
                &snapshot,
            )
            .await;

            // Store message in history for CHATHISTORY support
            let prefix_str = format!("{}!{}@{}", relay_from, snapshot.user, snapshot.ip);
            let stored_msg = StoredMessage {
                msgid,
                target: target_lower.clone(),
                sender: relay_from.to_string(),
                envelope: MessageEnvelope {
                    command: "PRIVMSG".to_string(),
                    prefix: prefix_str,
                    target: target.to_string(),
                    text: text.to_string(),
                    tags: None,
                },
                nanotime,
                account: None, // Relayed messages don't have an account
            };
            if let Err(e) = ctx
                .matrix
                .service_manager
                .history
                .store(target, stored_msg)
                .await
            {
                debug!(error = %e, "Failed to store relayed message in history");
            }

            debug!(
                relay_from = %relay_from,
                target = %target,
                "RELAYMSG relayed to channel"
            );
        } else {
            // User target
            let target_lower = irc_to_lower(target);

            // Check if user exists
            if ctx.matrix.user_manager.nicks.get(&target_lower).is_none() {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        ctx.state.nick.clone(),
                        target.to_string(),
                        "No such nick".to_string(),
                    ],
                );
                let reply = with_label(reply, ctx.label.as_deref());
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            let route_opts = RouteOptions {
                send_away_reply: true,
                status_prefix: None,
            };

            let _ = route_to_user_with_snapshot(
                ctx,
                &target_lower,
                relayed_msg,
                &route_opts,
                None,
                None,
                &snapshot,
            )
            .await;

            debug!(
                relay_from = %relay_from,
                target = %target,
                "RELAYMSG relayed to user"
            );
        }

        // Echo already sent above; suppress framework-level ACK.
        ctx.suppress_labeled_ack = true;

        Ok(())
    }
}
