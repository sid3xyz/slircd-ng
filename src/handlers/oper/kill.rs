//! KILL command handler for operator-initiated disconnections.
//!
//! Allows operators to forcibly disconnect a user from the server.
//! Uses capability-based authorization (Innovation 4).

use super::super::{
    Context, HandlerResult, PostRegHandler, resolve_nick_or_nosuchnick, user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::{require_arg_or_reply, require_oper_cap};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};
use std::sync::Arc;

/// Handler for KILL command.
///
/// `KILL nickname :reason`
///
/// Disconnects a user from the network. Requires operator privileges.
/// Uses capability-based authorization (Innovation 4).
/// # RFC 2812 §3.7.1
///
/// Kill message - Removes client from network (operator only).
///
/// **Specification:** [RFC 2812 §3.7.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.7.1)
///
/// **Compliance:** 1/1 irctest pass
pub struct KillHandler;

#[async_trait]
impl PostRegHandler for KillHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let Some(target_nick) = require_arg_or_reply!(ctx, msg, 0, "KILL") else {
            return Ok(());
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Get killer's identity
        let Some((killer_nick, killer_user, killer_host)) =
            user_mask_from_state(ctx, ctx.uid).await
        else {
            return Ok(());
        };

        // Request KILL capability from authority (Innovation 4)
        let Some(_kill_cap) = require_oper_cap!(ctx, "KILL", request_kill_cap) else {
            return Ok(());
        };

        let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "KILL", target_nick).await? else {
            return Ok(());
        };

        let quit_reason = format!("Killed by {killer_nick} ({reason})");

        // Check if target is local or remote
        let is_local = target_uid.starts_with(ctx.matrix.server_id.as_str());

        if is_local {
            // Local User: Disconnect locally
            let target_sender = ctx.matrix.user_manager.get_first_sender(&target_uid);
            if let Some(target_sender) = target_sender {
                let error_msg = Message {
                    tags: None,
                    prefix: None,
                    command: Command::ERROR(format!(
                        "Closing Link: {} ({})",
                        target_nick, quit_reason
                    )),
                };
                let _ = target_sender.send(Arc::new(error_msg)).await;
            }

            // This broadcasts QUIT to local channels and S2S peers
            ctx.matrix.disconnect_user(&target_uid, &quit_reason).await;

            tracing::info!(target: "audit", killer = %killer_nick, target = %target_nick, reason = %reason, "KILL command executed (Local)");
        } else {
            // Remote User: Route KILL to the owning server
            tracing::info!(target: "audit", killer = %killer_nick, target = %target_nick, uid = %target_uid, reason = %reason, "Routing KILL to remote server");

            let kill_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(
                    killer_nick.clone(),
                    killer_user.clone(),
                    killer_host.clone(),
                )),
                command: Command::KILL(target_nick.to_string(), quit_reason.clone()),
            };

            let routed = ctx
                .matrix
                .sync_manager
                .route_to_remote_user(&target_uid, Arc::new(kill_msg))
                .await;

            if !routed {
                // If routing failed (no path), send error to operator
                // In a split network, we might want to forcefully remove them locally?
                // But generally we just inform the oper.
                let _ = ctx
                    .sender
                    .send(Message::from(Command::NOTICE(
                        killer_nick.clone(),
                        format!("*** Could not route KILL to remote user {}", target_nick),
                    )))
                    .await;
            }
        }

        // Send snomask 'k' (local broadcast of the attempt)
        ctx.matrix
            .user_manager
            .send_snomask(
                'k',
                &format!(
                    "Received KILL message for {}. From {} Path: {}!{}@{} ({})",
                    target_nick, killer_nick, killer_nick, killer_user, killer_host, reason
                ),
            )
            .await;

        // Note: We do NOT echo the KILL back to the sender here manually,
        // because for local users the QUIT will come back, and for remote users
        // we might get a KILL broadcast back or a QUIT.
        // Standard IRC clients usually just see the QUIT.

        Ok(())
    }
}
