use crate::config::LinkBlock;
use crate::state::Matrix;
use dashmap::DashMap;
use slirc_proto::sync::ServerId;
use slirc_proto::{Command, Message};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::info;

use super::burst;
use super::handshake;
use super::link::LinkState;
use super::network;
use super::topology::{ServerInfo, TopologyGraph};

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
    /// S2S rate limiter for flood protection.
    pub rate_limiter: Arc<crate::security::rate_limit::S2SRateLimiter>,
}

impl SyncManager {
    pub fn new(
        local_id: ServerId,
        local_name: String,
        local_desc: String,
        configured_links: Vec<LinkBlock>,
        rate_limit_config: &crate::config::RateLimitConfig,
    ) -> Self {
        Self {
            local_id,
            local_name,
            local_desc,
            configured_links,
            links: Arc::new(DashMap::new()),
            topology: Arc::new(TopologyGraph::new()),
            rate_limiter: Arc::new(crate::security::rate_limit::S2SRateLimiter::new(
                rate_limit_config,
            )),
        }
    }

    /// Route a message to a remote user.
    ///
    /// Resolves the target server from the UID, finds the next hop,
    /// and sends the message via the appropriate link.
    pub async fn route_to_remote_user(&self, target_uid: &str, msg: Arc<Message>) -> bool {
        // 1. Extract Server ID from UID (first 3 chars)
        if target_uid.len() < 3 {
            tracing::warn!("Cannot route to invalid UID: {}", target_uid);
            return false;
        }
        let target_sid_str = &target_uid[0..3];
        let target_sid = ServerId::new(target_sid_str.to_string());

        // 2. Check if target is local
        if target_sid == self.local_id {
            tracing::warn!("Attempted to route local user {} via S2S", target_uid);
            return false;
        }

        // 3. Find next hop (must be a directly connected peer)
        if let Some(link) = self.get_next_hop(&target_sid) {
            let _ = link.tx.send(msg).await;
            crate::metrics::inc_distributed_messages_routed(
                self.local_id.as_str(),
                target_sid.as_str(),
                "success",
            );
            true
        } else {
            tracing::warn!(
                "No route to server {} (for user {})",
                target_sid.as_str(),
                target_uid
            );
            crate::metrics::inc_distributed_messages_routed(
                self.local_id.as_str(),
                target_sid.as_str(),
                "no_route",
            );
            false
        }
    }

    pub fn start_heartbeat(&self, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Instant::now();

                        // Collect SIDs to avoid holding lock while sending
                        let mut peers_to_ping = Vec::new();
                        let mut peers_to_drop = Vec::new();

                        {
                            for entry in manager.links.iter() {
                                let sid = entry.key().clone();
                                let link = entry.value();

                                if now.duration_since(link.last_pong) > Duration::from_secs(90) {
                                    info!("Peer {} timed out", sid.as_str());
                                    peers_to_drop.push(sid);
                                } else {
                                    peers_to_ping.push(sid);
                                }
                            }
                        }

                        for sid in peers_to_drop {
                            manager.links.remove(&sid);
                        }

                        for sid in peers_to_ping {
                            if let Some(mut link) = manager.links.get_mut(&sid) {
                                link.last_ping = now;
                                let ping = Command::PING(manager.local_id.as_str().to_string(), None);
                                let _ = link.tx.send(Arc::new(Message::from(ping))).await;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("S2S heartbeat stopping due to shutdown");
                        break;
                    }
                }
            }
        });
    }

    /// Starts the inbound S2S listener for accepting connections from remote servers.
    pub fn start_inbound_listener(
        &self,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        s2s_tls: Option<crate::config::S2STlsConfig>,
        s2s_listen: Option<std::net::SocketAddr>,
    ) {
        network::start_inbound_listener(self.clone(), matrix, registry, db, s2s_tls, s2s_listen);
    }

    /// Initiates an outbound connection.
    pub fn connect_to_peer(
        &self,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        config: LinkBlock,
    ) {
        network::connect_to_peer(self.clone(), matrix, registry, db, config);
    }

    /// Get a peer connection for a given server ID.
    pub fn get_peer_for_server(&self, sid: &ServerId) -> Option<LinkState> {
        self.links.get(sid).map(|l| l.value().clone())
    }

    /// Register a direct peer connection for the sync loop.
    pub async fn register_peer(
        &self,
        sid: ServerId,
        name: String,
        hopcount: u32,
        info: String,
    ) -> mpsc::Receiver<Arc<Message>> {
        let peer_sid = sid.clone();
        let (tx, rx) = mpsc::channel(1000);
        self.links.insert(
            peer_sid.clone(),
            LinkState {
                tx,
                state: handshake::HandshakeState::Synced,
                name: name.clone(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
                connected_at: Instant::now(),
                bytes_sent: Arc::new(AtomicU64::new(0)),
                bytes_recv: Arc::new(AtomicU64::new(0)),
            },
        );
        self.topology.servers.insert(
            peer_sid.clone(),
            ServerInfo {
                sid: peer_sid.clone(),
                name,
                info,
                hopcount,
                via: Some(peer_sid), // Direct peer routes through itself
            },
        );
        rx
    }

    pub async fn send_burst(&self, sid: &ServerId, matrix: &Matrix) {
        info!("Sending burst to {}", sid.as_str());
        let commands = burst::generate_burst(matrix, self.local_id.as_str(), sid.as_str()).await;

        let link = self.links.get(sid).map(|l| l.value().clone());
        if let Some(link) = link {
            for cmd in commands {
                let msg = Arc::new(Message::from(cmd));
                if let Err(e) = link.tx.send(msg).await {
                    tracing::error!("Failed to send burst command to {}: {}", sid.as_str(), e);
                    break;
                }
            }
        } else {
            tracing::warn!("Cannot send burst to {}: Link not found", sid.as_str());
        }
    }

    pub async fn remove_peer(&self, sid: &ServerId) {
        self.links.remove(sid);
    }

    /// Broadcast a message to all connected peers except the source.
    ///
    /// Implements split-horizon: we never echo a message back to its origin.
    pub async fn broadcast(&self, msg: Arc<Message>, source: Option<&ServerId>) {
        let peers: Vec<(ServerId, LinkState)> = self
            .links
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (peer_sid, link) in peers {
            // Split-horizon: don't send back to source
            if source.is_some_and(|src| src == &peer_sid) {
                tracing::debug!(peer = %peer_sid.as_str(), "Skipping source peer (split-horizon)");
                continue;
            }

            if let Err(e) = link.tx.send(msg.clone()).await {
                tracing::warn!(peer = %peer_sid.as_str(), error = %e, "Failed to send to peer");
            } else {
                tracing::debug!(peer = %peer_sid.as_str(), cmd = ?msg.command, "Sent to peer");
            }
        }
    }

    pub fn get_next_hop(&self, target: &ServerId) -> Option<LinkState> {
        use std::collections::HashSet;

        let mut current = target.clone();
        let mut visited = HashSet::new();

        loop {
            if let Some(link) = self.links.get(&current).map(|l| l.value().clone()) {
                return Some(link);
            }

            if !visited.insert(current.clone()) {
                return None;
            }

            let parent = self.topology.get_route(&current)?;
            current = parent;
        }
    }
}
