use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for server-to-server KICK propagation.
///
/// Format: KICK <channel> <target> :<reason>
/// Prefix identifies the kicker.
pub struct KickHandler;

#[async_trait]
impl ServerHandler for KickHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let channel = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(2).unwrap_or("Kicked");

        let sender = msg
            .prefix
            .as_ref()
            .map(|p| p.raw.to_string())
            .unwrap_or_else(|| ctx.state.sid.clone());

        let channel_lower = slirc_proto::irc_to_lower(channel);
        let channel_tx = ctx
            .matrix
            .channel_manager
            .channels
            .get(&channel_lower)
            .map(|s| s.value().clone());

        if let Some(channel_tx) = channel_tx {
            let event = ChannelEvent::RemoteKick {
                sender,
                target: target.to_string(),
                reason: reason.to_string(),
            };

            if let Err(e) = channel_tx.send(event).await {
                warn!(channel = %channel, error = %e, "Failed to send remote KICK to channel actor");
            }
        } else {
            warn!(channel = %channel, target = %target, "Received KICK for unknown channel");
        }

        Ok(())
    }
}
