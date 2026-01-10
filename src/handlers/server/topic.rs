use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for server-to-server TOPIC propagation.
///
/// Format: TOPIC <channel> <ts> :<topic>
/// Prefix identifies the setter (server or user).
pub struct TopicHandler;

#[async_trait]
impl ServerHandler for TopicHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let channel = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let ts_str = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let topic = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        let ts = ts_str
            .parse::<u64>()
            .map_err(|_| HandlerError::ProtocolError(format!("Invalid timestamp: {}", ts_str)))?;

        let setter = msg
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
            let event = ChannelEvent::RemoteTopic {
                ts,
                setter,
                topic: topic.to_string(),
            };
            if let Err(e) = channel_tx.send(event).await {
                warn!(channel = %channel, error = %e, "Failed to send remote TOPIC to channel actor");
            }
        } else {
            // If the channel doesn't exist locally yet, we ignore.
            // Channel state will be created via SJOIN during burst.
            warn!(channel = %channel, "Received TOPIC for unknown channel");
        }

        Ok(())
    }
}
