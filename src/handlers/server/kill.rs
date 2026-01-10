use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::dashmap_ext::DashMapExt;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef};
use std::sync::Arc;
use tracing::info;

/// Handler for the KILL command received from a remote server.
///
/// When a server sends KILL, we must:
/// 1. Disconnect the target user locally (if they exist)
/// 2. Propagate the KILL to other linked servers (split-horizon)
pub struct KillHandler;

#[async_trait]
impl ServerHandler for KillHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: :<source> KILL <target_uid> :<reason>
        let target_uid = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(1).unwrap_or("Killed");

        let source = msg
            .prefix
            .as_ref()
            .map(|p| p.raw.to_string())
            .unwrap_or_else(|| ctx.state.sid.clone());

        info!(
            source = %source,
            target = %target_uid,
            reason = %reason,
            "Received remote KILL"
        );

        // Check if we have the target user
        if ctx.matrix.user_manager.users.contains_key(target_uid) {
            // Send ERROR to the user before disconnecting
            let quit_reason = format!("Killed ({})", reason);

            if let Some(sender) = ctx.matrix.user_manager.senders.get_cloned(target_uid) {
                let error_msg = Message {
                    tags: None,
                    prefix: None,
                    command: Command::ERROR(format!("Closing Link: ({})", quit_reason)),
                };
                let _ = sender.send(Arc::new(error_msg)).await;
            }

            ctx.matrix
                .disconnect_user(&target_uid.to_string(), &quit_reason)
                .await;
        }

        // Propagate to other servers (split-horizon: exclude source)
        let source_sid = slirc_crdt::clock::ServerId::new(ctx.state.sid.clone());
        let kill_msg = Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new_from_str(&source)),
            command: Command::KILL(target_uid.to_string(), reason.to_string()),
        };
        ctx.matrix
            .sync_manager
            .broadcast(Arc::new(kill_msg), Some(&source_sid))
            .await;

        Ok(())
    }
}
