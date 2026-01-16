use crate::handlers::core::traits::{PreRegHandler, ServerHandler};
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::{ServerState, UnregisteredState};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use slirc_proto::sync::clock::ServerId;
use std::sync::Arc;
use tracing::{info, warn};

pub mod capab;
pub mod encap;
pub mod kick;
pub mod kill;
pub mod routing;
pub mod sid;
pub mod sjoin;
pub mod source;
pub mod svinfo;
pub mod tmode;
pub mod topic;
pub mod uid;

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

        // Verify password
        let link_block = ctx
            .matrix
            .config
            .links
            .iter()
            .find(|l| l.name == name)
            .ok_or(HandlerError::AccessDenied)?;

        if let Some(pass) = &ctx.state.pass_received {
            if pass != &link_block.password {
                warn!("Invalid password for server {}", name);
                return Err(HandlerError::AccessDenied);
            }
        } else {
            warn!("No password received for server {}", name);
            return Err(HandlerError::AccessDenied);
        }

        // Optional SID validation (prevents misrouting/misconfiguration).
        if let Some(expected_sid) = link_block.sid.as_deref()
            && expected_sid != sid
        {
            warn!(
                name = %name,
                expected_sid = %expected_sid,
                got_sid = %sid,
                "Server SID mismatch for configured link"
            );
            return Err(HandlerError::AccessDenied);
        }

        // Only send credentials if we are NOT the initiator.
        // Initiators send credentials in run_handshake_loop before receiving anything.
        if ctx.state.initiator_data.is_none() {
            // Send credentials
            // PASS <password> TS 6 :<sid>
            let pass_cmd = slirc_proto::Command::PassTs6 {
                password: link_block.password.clone(),
                sid: ctx.matrix.server_info.sid.as_str().to_string(),
            };
            ctx.sender
                .send(slirc_proto::Message::from(pass_cmd))
                .await?;

            // SERVER <name> <hopcount> <sid> <info>
            let server_cmd = slirc_proto::Command::SERVER(
                ctx.matrix.server_info.name.clone(),
                1,
                ctx.matrix.server_info.sid.as_str().to_string(),
                ctx.matrix.server_info.description.clone(),
            );
            ctx.sender
                .send(slirc_proto::Message::from(server_cmd))
                .await?;
        }

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

        // Split-horizon: don't send back to the direct peer we received this from.
        let peer_sid = ServerId::new(ctx.state.sid.clone());

        // Topology: record the immediate uplink/introducer (from prefix when available).
        let uplink_sid = crate::handlers::server::source::extract_source_sid(msg)
            .unwrap_or_else(|| peer_sid.clone());

        // 2. Update topology
        ctx.matrix.sync_manager.topology.add_server(
            sid.clone(),
            name.to_string(),
            info.to_string(),
            hopcount,
            Some(uplink_sid),
        );

        info!(
            "Learned about new server {} ({}) via {}",
            name, sid_str, ctx.state.name
        );

        // 3. Propagate to other peers (Split Horizon)
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
            .broadcast(Arc::new(out_msg), Some(&peer_sid))
            .await;

        Ok(())
    }
}
