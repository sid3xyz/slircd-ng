use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::{debug, warn};

/// Handler for the TB (Topic Burst) command.
///
/// Format: `:<sid> TB <channel> <ts> [setter] :<topic>`
///
/// TB is used during burst to synchronize channel topics.
pub struct TbHandler;

#[async_trait]
impl ServerHandler for TbHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Args: channel, ts, [setter], topic
        let channel = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let ts_str = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        let (setter, topic) = if msg.args().len() >= 4 {
            (
                msg.arg(2).ok_or(HandlerError::NeedMoreParams)?.to_string(),
                msg.arg(3).ok_or(HandlerError::NeedMoreParams)?.to_string(),
            )
        } else {
            let t = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
            (ctx.state.name.clone(), t.to_string())
        };

        let ts = ts_str
            .parse::<u64>()
            .map_err(|_| HandlerError::ProtocolError(format!("Invalid timestamp: {}", ts_str)))?;

        debug!(channel = %channel, topic = %topic, "Applying TB");

        let channel_lower = slirc_proto::irc_to_lower(channel);
        let channel_tx = ctx
            .matrix
            .channel_manager
            .channels
            .get(&channel_lower)
            .map(|s| s.value().clone());

        if let Some(channel_tx) = channel_tx {
            let event = ChannelEvent::RemoteTopic { ts, setter, topic };
            if let Err(e) = channel_tx.send(event).await {
                warn!(channel = %channel, error = %e, "Failed to send TB to channel actor");
            }
        } else {
            warn!(channel = %channel, "Received TB for unknown channel");
        }

        Ok(())
    }
}
