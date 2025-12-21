use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for the DELTA command (incremental state updates).
pub struct DeltaHandler;

#[async_trait]
impl ServerHandler for DeltaHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let delta_type = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let payload = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        tracing::info!(delta_type = %delta_type, from = %ctx.state.name, payload_len = payload.len(), "Received DELTA");

        match delta_type {
            "USER" => {
                tracing::info!(from = %ctx.state.name, "Processing USER DELTA");
                let user_crdt: slirc_crdt::user::UserCrdt = serde_json::from_str(payload)
                    .map_err(|e| {
                        warn!(error = %e, "Failed to parse USER DELTA payload");
                        HandlerError::ProtocolError("Invalid USER DELTA payload".to_string())
                    })?;
                ctx.matrix
                    .user_manager
                    .merge_user_crdt(user_crdt, Some(slirc_crdt::clock::ServerId::new(ctx.state.sid.clone())))
                    .await;
            }
            "CHANNEL" => {
                let channel_crdt: slirc_crdt::channel::ChannelCrdt = serde_json::from_str(payload)
                    .map_err(|e| {
                        warn!(error = %e, "Failed to parse CHANNEL DELTA payload");
                        HandlerError::ProtocolError("Invalid CHANNEL DELTA payload".to_string())
                    })?;
                ctx.matrix
                    .channel_manager
                    .merge_channel_crdt(
                        channel_crdt,
                        std::sync::Arc::downgrade(ctx.matrix),
                        Some(slirc_crdt::clock::ServerId::new(ctx.state.sid.clone())),
                    )
                    .await;
            }
            _ => {
                warn!(delta_type = %delta_type, "Unknown DELTA type");
            }
        }

        Ok(())
    }
}
