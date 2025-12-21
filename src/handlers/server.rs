use crate::handlers::core::traits::{PreRegHandler, ServerHandler};
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::{ServerState, UnregisteredState};
use async_trait::async_trait;
use slirc_crdt::clock::ServerId;
use slirc_proto::MessageRef;
use std::sync::Arc;
use tracing::{info, warn};

pub mod delta;
pub mod routing;

/// Handler for the SERVER command (server-to-server handshake).
pub struct ServerHandshakeHandler;

#[async_trait]
impl PreRegHandler for ServerHandshakeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        if msg.command_name() != "SERVER" {
            return Ok(());
        }

        let name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let hopcount = msg
            .arg(1)
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(HandlerError::NeedMoreParams)?;
        let sid = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
        let info = msg.arg(3).unwrap_or("");

        info!(
            name = %name,
            sid = %sid,
            hopcount = %hopcount,
            info = %info,
            "Received SERVER handshake"
        );

        // In a real implementation, we would verify the password (PASS command)
        // and check if the server is allowed to connect.
        // For now, we'll just accept it.

        // Transition to ServerState is handled by the lifecycle loop
        // when it sees that the connection has become a server.

        // We need a way to signal to the lifecycle loop that this is now a server.
        ctx.state.is_server_handshake = true;
        ctx.state.server_name = Some(name.to_string());
        ctx.state.server_sid = Some(sid.to_string());
        ctx.state.server_info = Some(info.to_string());
        ctx.state.server_hopcount = hopcount;

        Ok(())
    }
}

/// Handler for the BURST command (initial state synchronization).
pub struct BurstHandler;

#[async_trait]
impl ServerHandler for BurstHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let burst_type = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let payload = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        match burst_type {
            "USER" => {
                let user_crdt: slirc_crdt::user::UserCrdt =
                    serde_json::from_str(payload).map_err(|e| {
                        warn!(error = %e, "Failed to parse USER BURST payload");
                        HandlerError::ProtocolError("Invalid USER BURST payload".to_string())
                    })?;
                ctx.matrix
                    .user_manager
                    .merge_user_crdt(user_crdt, Some(ServerId::new(ctx.state.sid.clone())))
                    .await;
            }
            "CHANNEL" => {
                let channel_crdt: slirc_crdt::channel::ChannelCrdt = serde_json::from_str(payload)
                    .map_err(|e| {
                        warn!(error = %e, "Failed to parse CHANNEL BURST payload");
                        HandlerError::ProtocolError("Invalid CHANNEL BURST payload".to_string())
                    })?;
                ctx.matrix
                    .channel_manager
                    .merge_channel_crdt(
                        channel_crdt,
                        Arc::downgrade(ctx.matrix),
                        Some(ServerId::new(ctx.state.sid.clone())),
                    )
                    .await;
            }
            _ => {
                warn!(burst_type = %burst_type, "Unknown BURST type");
            }
        }

        Ok(())
    }
}

/// Handler for the DELTA command (incremental state updates).
#[allow(dead_code)]
pub struct DeltaHandler;

#[async_trait]
impl ServerHandler for DeltaHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let start = std::time::Instant::now();
        let delta_type = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let payload = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        match delta_type {
            "USER" => {
                let user_crdt: slirc_crdt::user::UserCrdt =
                    serde_json::from_str(payload).map_err(|e| {
                        warn!(error = %e, "Failed to parse USER DELTA payload");
                        HandlerError::ProtocolError("Invalid USER DELTA payload".to_string())
                    })?;
                ctx.matrix
                    .user_manager
                    .merge_user_crdt(user_crdt, Some(ServerId::new(ctx.state.sid.clone())))
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
                        Arc::downgrade(ctx.matrix),
                        Some(ServerId::new(ctx.state.sid.clone())),
                    )
                    .await;
            }
            _ => {
                warn!(delta_type = %delta_type, "Unknown DELTA type");
            }
        }

        let duration = start.elapsed().as_secs_f64();
        crate::metrics::DISTRIBUTED_SYNC_LATENCY
            .with_label_values(&[&ctx.state.sid])
            .observe(duration);

        Ok(())
    }
}

/// Handler for SERVER commands received from established peers (topology propagation).
pub struct ServerPropagationHandler;

#[async_trait]
impl ServerHandler for ServerPropagationHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // SERVER <name> <hopcount> <sid> <info>
        let name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let hopcount = msg
            .arg(1)
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(HandlerError::NeedMoreParams)?;
        let sid_str = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
        let info = msg.arg(3).unwrap_or("");

        let sid = ServerId::new(sid_str.to_string());

        // 1. Check if we already know this server
        if ctx.matrix.sync_manager.topology.servers.contains_key(&sid) {
            // Already known. Maybe update info?
            // For now, ignore to prevent loops if not strictly checked
            return Ok(());
        }

        // 2. Register route
        // The peer we received this from is the next hop
        let peer_sid = ServerId::new(ctx.state.sid.clone());

        ctx.matrix
            .sync_manager
            .register_route(sid.clone(), peer_sid.clone());

        // 3. Update Topology
        let server_info = crate::sync::ServerInfo {
            sid: sid.clone(),
            name: name.to_string(),
            hopcount,
            info: info.to_string(),
            via: Some(peer_sid.clone()),
        };
        ctx.matrix
            .sync_manager
            .topology
            .servers
            .insert(sid.clone(), server_info);

        info!(
            "Learned about new server {} ({}) via {}",
            name, sid_str, ctx.state.name
        );

        // 4. Propagate to other peers (Split Horizon)
        // We increment hopcount
        let new_hopcount = hopcount + 1;
        let cmd = slirc_proto::Command::SERVER(
            name.to_string(),
            new_hopcount,
            sid_str.to_string(),
            info.to_string(),
        );
        let out_msg = slirc_proto::Message::from(cmd);

        ctx.matrix
            .sync_manager
            .broadcast(out_msg, Some(&peer_sid))
            .await;

        Ok(())
    }
}
