//! Sync Module - Server-to-Server Synchronization.
//!
//! This module manages the distributed state of the IRC network.
//! It handles server linking, handshake, and CRDT state replication.

pub mod handshake;
pub mod protocol;
pub mod burst;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod test_protocol;

use dashmap::DashMap;
use slirc_crdt::clock::ServerId;
use slirc_crdt::user::UserCrdt;
use slirc_crdt::channel::ChannelCrdt;
use crate::state::observer::StateObserver;
use crate::state::{UserManager, ChannelManager, Matrix};
use slirc_proto::{Message, Command};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::info;
use tokio_util::codec::{Framed, LinesCodec};
use futures_util::{SinkExt, StreamExt};
use crate::sync::handshake::{HandshakeMachine, HandshakeState};

use crate::config::LinkBlock;

/// Represents the state of a link to a peer server.
#[derive(Debug)]
pub struct LinkState {
    /// The channel to send messages to this peer.
    pub tx: mpsc::Sender<Message>,
    /// The current handshake state.
    pub state: handshake::HandshakeState,
    /// The name of the peer server.
    pub name: String,
}

impl Clone for LinkState {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            state: self.state.clone(),
            name: self.name.clone(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub sid: ServerId,
    pub name: String,
    pub info: String,
    pub hopcount: u32,
    pub parent: Option<ServerId>,
}

#[derive(Debug, Clone)]
pub struct TopologyGraph {
    pub servers: DashMap<ServerId, ServerInfo>,
}

impl TopologyGraph {
    pub fn new() -> Self {
        Self {
            servers: DashMap::new(),
        }
    }
}

/// Manages server-to-server synchronization and peer connections.
#[derive(Clone)]
pub struct SyncManager {
    /// Local Server ID.
    pub local_id: ServerId,
    pub local_name: String,
    pub local_desc: String,
    pub configured_links: Vec<LinkBlock>,
    /// Connected peers (Direct connections).
    pub links: Arc<DashMap<ServerId, LinkState>>,
    /// Network topology.
    pub topology: Arc<TopologyGraph>,
}

impl SyncManager {
    pub fn new(local_id: ServerId, local_name: String, local_desc: String, configured_links: Vec<LinkBlock>) -> Self {
        Self {
            local_id,
            local_name,
            local_desc,
            configured_links,
            links: Arc::new(DashMap::new()),
            topology: Arc::new(TopologyGraph::new()),
        }
    }

    /// Spawns a handler for an incoming server link.
    #[allow(dead_code)]
    pub fn handle_inbound_connection(&self, matrix: Arc<Matrix>, stream: TcpStream) {
        let manager = self.clone();
        let matrix = matrix.clone();
        tokio::spawn(async move {
            info!("Handling inbound server connection");
            let mut framed = Framed::new(stream, LinesCodec::new());

            let mut machine = HandshakeMachine::new(
                manager.local_id.clone(),
                manager.local_name.clone(),
                manager.local_desc.clone(),
            );
            machine.transition(HandshakeState::InboundReceived);

            while let Some(Ok(line)) = framed.next().await {
                let msg = match line.parse::<Message>() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                match machine.step(msg.command, &manager.configured_links) {
                    Ok(responses) => {
                        for resp in responses {
                            if let Err(e) = framed.send(Message::from(resp).to_string()).await {
                                tracing::error!("Failed to send response: {}", e);
                                return;
                            }
                        }

                        if machine.state == HandshakeState::Bursting {
                            info!("Handshake complete (Inbound). Remote: {:?}", machine.remote_name);

                            // Generate Burst
                            let burst = burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                            for cmd in burst {
                                if let Err(e) = framed.send(Message::from(cmd).to_string()).await {
                                    tracing::error!("Failed to send burst: {}", e);
                                    return;
                                }
                            }

                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Handshake error: {:?}", e);
                        return;
                    }
                }
            }

            let handler = protocol::IncomingCommandHandler::new(matrix.clone());
            while let Some(Ok(line)) = framed.next().await {
                let msg = match line.parse::<Message>() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Failed to parse inbound message: {}", e);
                        continue;
                    }
                };

                if let Err(e) = handler.handle_command(msg.command).await {
                    tracing::error!("Protocol error from peer: {}", e);
                    return;
                }
            }
        });
    }

    /// Initiates an outbound connection.
    pub fn connect_to_peer(&self, matrix: Arc<Matrix>, config: LinkBlock) {
        let manager = self.clone();
        let matrix = matrix.clone();
        tokio::spawn(async move {
            info!("Connecting to peer {}", config.hostname);
            let stream = match TcpStream::connect(format!("{}:{}", config.hostname, config.port)).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to connect to {}: {}", config.hostname, e);
                    return;
                }
            };

            let mut framed = Framed::new(stream, LinesCodec::new());

            let mut machine = HandshakeMachine::new(
                manager.local_id.clone(),
                manager.local_name.clone(),
                manager.local_desc.clone(),
            );
            machine.transition(HandshakeState::OutboundInitiated);

            // Send initial PASS and SERVER
            let pass_cmd = Command::Raw(
                "PASS".to_string(),
                vec![config.password.clone(), "TS=6".to_string(), manager.local_id.as_str().to_string()]
            );
            let server_cmd = Command::SERVER(
                manager.local_name.clone(),
                1,
                manager.local_id.as_str().to_string(),
                manager.local_desc.clone(),
            );

            if let Err(e) = framed.send(Message::from(pass_cmd).to_string()).await {
                 tracing::error!("Failed to send PASS: {}", e);
                 return;
            }
            if let Err(e) = framed.send(Message::from(server_cmd).to_string()).await {
                 tracing::error!("Failed to send SERVER: {}", e);
                 return;
            }

            let links = vec![config.clone()];

            while let Some(Ok(line)) = framed.next().await {
                let msg = match line.parse::<Message>() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                match machine.step(msg.command, &links) {
                    Ok(responses) => {
                        for resp in responses {
                            if let Err(e) = framed.send(Message::from(resp).to_string()).await {
                                tracing::error!("Failed to send response: {}", e);
                                return;
                            }
                        }

                        if machine.state == HandshakeState::Bursting {
                            info!("Handshake complete (Outbound). Remote: {:?}", machine.remote_name);

                            // Generate Burst
                            let burst = burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                            for cmd in burst {
                                if let Err(e) = framed.send(Message::from(cmd).to_string()).await {
                                    tracing::error!("Failed to send burst: {}", e);
                                    return;
                                }
                            }

                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Handshake error with {}: {:?}", config.hostname, e);
                        return;
                    }
                }
            }

            let handler = protocol::IncomingCommandHandler::new(matrix.clone());
            while let Some(Ok(line)) = framed.next().await {
                let msg = match line.parse::<Message>() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Failed to parse inbound message: {}", e);
                        continue;
                    }
                };

                if let Err(e) = handler.handle_command(msg.command).await {
                    tracing::error!("Protocol error from peer: {}", e);
                    return;
                }
            }
        });
    }

    /// Get a peer connection for a given server ID.
    pub fn get_peer_for_server(&self, sid: &ServerId) -> Option<LinkState> {
        self.links.get(sid).map(|l| l.clone())
    }

    // Legacy/Stub methods to satisfy existing code
    pub async fn register_peer(&self, sid: ServerId, name: String, hopcount: u32, info: String) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel(1000);
        self.links.insert(sid.clone(), LinkState {
            tx,
            state: handshake::HandshakeState::Synced,
            name: name.clone(),
        });
        self.topology.servers.insert(sid.clone(), ServerInfo {
            sid,
            name,
            info,
            hopcount,
            parent: None, // Direct peer
        });
        rx
    }

    pub async fn send_burst(&self, sid: &ServerId, _user_manager: &UserManager, _channel_manager: &ChannelManager) {
        info!("Sending burst to {}", sid.as_str());
    }

    pub async fn remove_peer(&self, sid: &ServerId) {
        self.links.remove(sid);
    }

    pub async fn broadcast(&self, _msg: Message, _source: Option<&ServerId>) {
        // Stub
    }

    pub fn get_next_hop(&self, target: &ServerId) -> Option<LinkState> {
        // Stub: just check if it's a direct link for now
        self.links.get(target).map(|l| l.clone())
    }

    pub fn register_route(&self, _target: ServerId, _via: ServerId) {
        // Stub
    }
}

impl StateObserver for SyncManager {
    fn on_user_update(&self, user: &UserCrdt, source: Option<ServerId>) {
        // Placeholder for CRDT replication
        info!(uid = %user.uid, source = ?source, "SyncManager: on_user_update (stub)");
    }

    fn on_user_quit(&self, uid: &str, reason: &str, _source: Option<ServerId>) {
        // Placeholder for CRDT replication
        info!(uid = %uid, reason = %reason, "SyncManager: on_user_quit (stub)");
    }

    fn on_channel_update(&self, channel: &ChannelCrdt, source: Option<ServerId>) {
        info!(channel = %channel.name, source = ?source, "SyncManager: on_channel_update (stub)");
    }

    fn on_channel_destroy(&self, name: &str, source: Option<ServerId>) {
        info!(channel = %name, source = ?source, "SyncManager: on_channel_destroy (stub)");
    }
}
