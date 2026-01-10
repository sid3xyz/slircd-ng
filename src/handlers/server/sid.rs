use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use async_trait::async_trait;
use slirc_crdt::clock::ServerId;
use slirc_proto::MessageRef;
use tracing::info;

use crate::handlers::server::source::extract_source_sid;

/// Handler for the SID command (Server ID).
///
/// SID introduces a new server to the network topology.
pub struct SidHandler;

#[async_trait]
impl ServerHandler for SidHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: SID <server_name> <hopcount> <sid> <info>

        let name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let hopcount_str = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let sid_str = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
        let info_str = msg.arg(3).unwrap_or("");

        let hopcount = hopcount_str.parse::<u32>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid hopcount: {}", hopcount_str))
        })?;

        let sid = ServerId::new(sid_str.to_string());

        // Topology: record the immediate uplink/introducer (from prefix when available).
        let via = extract_source_sid(msg).unwrap_or_else(|| ServerId::new(ctx.state.sid.clone()));

        ctx.matrix.sync_manager.topology.add_server(
            sid.clone(),
            name.to_string(),
            info_str.to_string(),
            hopcount,
            Some(via),
        );

        info!(sid = %sid.as_str(), name = %name, via = %ctx.state.sid, "Registered remote server via SID");

        Ok(())
    }
}
