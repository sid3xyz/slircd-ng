use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for the TMODE command (Timestamped Mode).
///
/// TMODE is used by servers to propagate mode changes with a timestamp
/// for conflict resolution (Last-Write-Wins).
pub struct TModeHandler;

#[async_trait]
impl ServerHandler for TModeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: TMODE <timestamp> <channel> <modes> [args...]
        let ts_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let channel_name = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let modes = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        let ts = ts_str
            .parse::<u64>()
            .map_err(|_| HandlerError::ProtocolError(format!("Invalid timestamp: {}", ts_str)))?;

        // Collect mode arguments
        let mut mode_args = Vec::new();
        let mut arg_idx = 3;
        while let Some(arg) = msg.arg(arg_idx) {
            mode_args.push(arg.to_string());
            arg_idx += 1;
        }

        // Find channel
        let tx = ctx
            .matrix
            .channel_manager
            .channels
            .get(channel_name)
            .map(|t| t.value().clone());

        if let Some(tx) = tx {
            // Send to actor
            let event = ChannelEvent::RemoteMode {
                ts,
                setter: msg
                    .prefix
                    .as_ref()
                    .map(|p| p.raw.to_string())
                    .unwrap_or_else(|| ctx.state.sid.clone()),
                modes: modes.to_string(),
                args: mode_args,
            };

            if let Err(e) = tx.send(event).await {
                warn!(channel = %channel_name, error = %e, "Failed to send TMODE to channel actor");
            }
        } else {
            // Channel doesn't exist locally.
            // In a full mesh, we might need to create it or ignore it.
            // For TMODE, if we don't have the channel, we usually ignore it
            // because TMODE implies the channel exists.
            // However, if we are a hub, we might need to propagate it even if we don't have members.
            // But slircd-ng's current architecture seems to rely on the actor for state.
            // If the actor doesn't exist, we can't store the mode.
            warn!(channel = %channel_name, "Received TMODE for unknown channel");
        }

        Ok(())
    }
}
