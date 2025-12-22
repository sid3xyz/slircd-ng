use crate::config::LinkBlock;
use crate::metrics::{S2S_BYTES_RECEIVED, S2S_BYTES_SENT, S2S_COMMANDS};
use crate::state::{ChannelManager, Matrix, UserManager};
use crate::sync::burst;
use crate::sync::handshake::{self, HandshakeMachine, HandshakeState};
use crate::sync::link::LinkState;
use crate::sync::protocol;
use crate::sync::split;
use crate::sync::stream::S2SStream;
use crate::sync::tls::DangerousNoVerifier;
use crate::sync::topology::{ServerInfo, TopologyGraph};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use slirc_crdt::clock::ServerId;
use slirc_proto::{Command, Message};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::info;

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
    pub fn new(
        local_id: ServerId,
        local_name: String,
        local_desc: String,
        configured_links: Vec<LinkBlock>,
    ) -> Self {
        Self {
            local_id,
            local_name,
            local_desc,
            configured_links,
            links: Arc::new(DashMap::new()),
            topology: Arc::new(TopologyGraph::new()),
        }
    }

    /// Upgrades a TCP stream to TLS for outbound connections.
    ///
    /// # Arguments
    /// * `tcp_stream` - The established TCP connection
    /// * `hostname` - The remote hostname (used for SNI and certificate verification)
    /// * `verify_cert` - Whether to verify the remote certificate
    async fn upgrade_to_tls(
        tcp_stream: TcpStream,
        hostname: &str,
        verify_cert: bool,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>, Box<dyn std::error::Error + Send + Sync>>
    {
        use tokio_rustls::TlsConnector;
        use tokio_rustls::rustls::pki_types::ServerName;
        use tokio_rustls::rustls::{ClientConfig, RootCertStore};

        let root_store = if verify_cert {
            // Load system root certificates
            let mut roots = RootCertStore::empty();
            let certs = rustls_native_certs::load_native_certs();
            for cert in certs.certs {
                if let Err(e) = roots.add(cert) {
                    tracing::warn!("Failed to add root cert: {}", e);
                }
            }
            if !certs.errors.is_empty() {
                for e in &certs.errors {
                    tracing::warn!("Error loading native certs: {}", e);
                }
            }
            roots
        } else {
            // Empty root store with custom verifier that accepts all certs
            RootCertStore::empty()
        };

        let config = if verify_cert {
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        } else {
            // Dangerous: Skip certificate verification (for testing/self-signed certs only)
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(DangerousNoVerifier))
                .with_no_client_auth()
        };

        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(hostname.to_string())?;

        let tls_stream = connector.connect(server_name, tcp_stream).await?;
        info!(hostname = %hostname, verify = verify_cert, "TLS handshake completed for S2S link");

        Ok(tls_stream)
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

        // 3. Find next hop
        let next_hop = self.topology.get_route(&target_sid).unwrap_or(target_sid.clone());

        // 4. Send to link
        if let Some(link) = self.links.get(&next_hop) {
            let _ = link.tx.send(msg).await;
            crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                .with_label_values(&[self.local_id.as_str(), target_sid.as_str(), "success"])
                .inc();
            true
        } else {
            tracing::warn!("No route to server {} (for user {})", target_sid.as_str(), target_uid);
            crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                .with_label_values(&[self.local_id.as_str(), target_sid.as_str(), "no_route"])
                .inc();
            false
        }
    }

    pub fn start_heartbeat(&self) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
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
        });
    }

    /// Spawns a handler for an incoming server link.
    #[allow(dead_code)]
    pub fn handle_inbound_connection(&self, matrix: Arc<Matrix>, stream: S2SStream) {
        let manager = self.clone();
        let matrix = matrix.clone();
        let is_tls = stream.is_tls();
        tokio::spawn(async move {
            info!(tls = is_tls, "Handling inbound server connection");
            let mut framed = Framed::new(stream, LinesCodec::new());

            let mut machine = HandshakeMachine::new(
                manager.local_id.clone(),
                manager.local_name.clone(),
                manager.local_desc.clone(),
            );
            machine.transition(HandshakeState::InboundReceived);

            // Track remote server info for netsplit handling
            let mut remote_sid: Option<ServerId> = None;
            let mut remote_name: Option<String> = None;

            while let Some(Ok(line)) = framed.next().await {
                let msg = match line.parse::<Message>() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                // Loop detection for SERVER command
                if let Command::SERVER(name, _, sid, _) = &msg.command {
                    let sid_obj = ServerId::new(sid.clone());
                    if manager.topology.servers.contains_key(&sid_obj) {
                        tracing::error!("Loop detected during handshake: {} ({})", name, sid);
                        let _ = framed
                            .send(
                                Message::from(Command::ERROR(format!(
                                    "Loop detected: {} ({})",
                                    name, sid
                                )))
                                .to_string(),
                            )
                            .await;
                        return;
                    }
                }

                match machine.step(msg.command, &manager.configured_links) {
                    Ok(responses) => {
                        for resp in responses {
                            if let Err(e) = framed.send(Message::from(resp).to_string()).await {
                                tracing::error!("Failed to send response: {}", e);
                                return;
                            }
                        }

                        if machine.state == HandshakeState::Bursting {
                            info!(
                                "Handshake complete (Inbound). Remote: {:?}",
                                machine.remote_name
                            );

                            // Capture remote server info
                            remote_sid = machine.remote_sid.clone();
                            remote_name = machine.remote_name.clone();

                            // Generate Burst
                            let burst =
                                burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                            for cmd in burst {
                                if let Err(e) = framed.send(Message::from(cmd).to_string()).await {
                                    tracing::error!("Failed to send burst: {}", e);
                                    // Connection failed, handle netsplit if we had a peer
                                    if let Some(sid) = &remote_sid {
                                        let rn = remote_name.as_deref().unwrap_or("unknown");
                                        split::handle_netsplit(
                                            &matrix,
                                            sid,
                                            &manager.local_name,
                                            rn,
                                        )
                                        .await;
                                    }
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

            // Register link
            let (tx, mut rx) = mpsc::channel::<Arc<Message>>(100);
            if let Some(sid) = &remote_sid {
                manager.links.insert(
                    sid.clone(),
                    LinkState {
                        tx,
                        state: handshake::HandshakeState::Synced,
                        name: remote_name.clone().unwrap_or_default(),
                        last_pong: Instant::now(),
                        last_ping: Instant::now(),
                        connected_at: Instant::now(),
                    },
                );
            }

            let handler = protocol::IncomingCommandHandler::new(matrix.clone());

            loop {
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Some(m) => {
                                let s = m.as_ref().to_string();
                                if let Some(sid) = &remote_sid {
                                    S2S_BYTES_SENT.with_label_values(&[sid.as_str()]).inc_by(s.len() as u64 + 2); // +2 for \r\n
                                }
                                if let Err(e) = framed.send(s).await {
                                     tracing::error!("Failed to send message to peer: {}", e);
                                     break;
                                }
                            }
                            None => {
                                info!("Link channel closed (timeout or removal), closing connection");
                                break;
                            }
                        }
                    }
                    result = framed.next() => {
                        match result {
                            Some(Ok(line)) => {
                                if let Some(sid) = &remote_sid {
                                    S2S_BYTES_RECEIVED.with_label_values(&[sid.as_str()]).inc_by(line.len() as u64 + 2); // +2 for \r\n
                                }
                                let msg = match line.parse::<Message>() {
                                    Ok(m) => m,
                                    Err(e) => {
                                        tracing::warn!("Failed to parse inbound message: {}", e);
                                        continue;
                                    }
                                };

                                #[allow(clippy::collapsible_if)]
                                if let Some(sid) = &remote_sid {
                                    S2S_COMMANDS.with_label_values(&[sid.as_str(), msg.command.name()]).inc();
                                    if let Err(e) = handler.handle_message(msg, &manager, sid).await {
                                        tracing::error!("Protocol error from peer: {}", e);
                                        break;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                tracing::error!("Stream error: {}", e);
                                break;
                            }
                            None => {
                                info!("Connection closed by peer");
                                break;
                            }
                        }
                    }
                }
            }

            // Connection ended - handle netsplit
            if let Some(sid) = remote_sid {
                let rn = remote_name.as_deref().unwrap_or("unknown");
                info!(remote_sid = %sid.as_str(), "Peer disconnected, initiating netsplit cleanup");
                split::handle_netsplit(&matrix, &sid, &manager.local_name, rn).await;
            }
        });
    }

    /// Initiates an outbound connection.
    pub fn connect_to_peer(&self, matrix: Arc<Matrix>, config: LinkBlock) {
        let manager = self.clone();
        let matrix = matrix.clone();
        tokio::spawn(async move {
            info!(hostname = %config.hostname, port = config.port, tls = config.tls, "Connecting to peer");

            // Establish TCP connection
            let tcp_stream =
                match TcpStream::connect(format!("{}:{}", config.hostname, config.port)).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to connect to {}: {}", config.hostname, e);
                        return;
                    }
                };

            // Upgrade to TLS if configured
            let stream: S2SStream = if config.tls {
                match Self::upgrade_to_tls(tcp_stream, &config.hostname, config.verify_cert).await {
                    Ok(tls_stream) => S2SStream::TlsClient(tls_stream),
                    Err(e) => {
                        tracing::error!("TLS handshake failed with {}: {}", config.hostname, e);
                        return;
                    }
                }
            } else {
                S2SStream::Plain(tcp_stream)
            };

            let mut framed = Framed::new(stream, LinesCodec::new());

            let mut machine = HandshakeMachine::new(
                manager.local_id.clone(),
                manager.local_name.clone(),
                manager.local_desc.clone(),
            );
            machine.transition(HandshakeState::OutboundInitiated);

            // Track remote server info for netsplit handling
            let mut remote_sid: Option<ServerId> = None;
            let mut remote_name: Option<String> = None;

            // Send initial PASS and SERVER
            let pass_cmd = Command::Raw(
                "PASS".to_string(),
                vec![
                    config.password.clone(),
                    "TS=6".to_string(),
                    manager.local_id.as_str().to_string(),
                ],
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

                // Loop detection for SERVER command
                if let Command::SERVER(name, _, sid, _) = &msg.command {
                    let sid_obj = ServerId::new(sid.clone());
                    if manager.topology.servers.contains_key(&sid_obj) {
                        tracing::error!("Loop detected during handshake: {} ({})", name, sid);
                        let _ = framed
                            .send(
                                Message::from(Command::ERROR(format!(
                                    "Loop detected: {} ({})",
                                    name, sid
                                )))
                                .to_string(),
                            )
                            .await;
                        return;
                    }
                }

                match machine.step(msg.command, &links) {
                    Ok(responses) => {
                        for resp in responses {
                            if let Err(e) = framed.send(Message::from(resp).to_string()).await {
                                tracing::error!("Failed to send response: {}", e);
                                return;
                            }
                        }

                        if machine.state == HandshakeState::Bursting {
                            info!(
                                "Handshake complete (Outbound). Remote: {:?}",
                                machine.remote_name
                            );

                            // Capture remote server info
                            remote_sid = machine.remote_sid.clone();
                            remote_name = machine.remote_name.clone();

                            // Generate Burst
                            let burst =
                                burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                            for cmd in burst {
                                if let Err(e) = framed.send(Message::from(cmd).to_string()).await {
                                    tracing::error!("Failed to send burst: {}", e);
                                    // Connection failed, handle netsplit if we had a peer
                                    if let Some(sid) = &remote_sid {
                                        let rn = remote_name.as_deref().unwrap_or("unknown");
                                        split::handle_netsplit(
                                            &matrix,
                                            sid,
                                            &manager.local_name,
                                            rn,
                                        )
                                        .await;
                                    }
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

            // Register link
            let (tx, mut rx) = mpsc::channel::<Arc<Message>>(100);
            if let Some(sid) = &remote_sid {
                manager.links.insert(
                    sid.clone(),
                    LinkState {
                        tx,
                        state: handshake::HandshakeState::Synced,
                        name: remote_name.clone().unwrap_or_default(),
                        last_pong: Instant::now(),
                        last_ping: Instant::now(),
                        connected_at: Instant::now(),
                    },
                );
            }

            let handler = protocol::IncomingCommandHandler::new(matrix.clone());

            loop {
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Some(m) => {
                                let s = m.as_ref().to_string();
                                if let Some(sid) = &remote_sid {
                                    S2S_BYTES_SENT.with_label_values(&[sid.as_str()]).inc_by(s.len() as u64 + 2); // +2 for \r\n
                                }
                                if let Err(e) = framed.send(s).await {
                                     tracing::error!("Failed to send message to peer: {}", e);
                                     break;
                                }
                            }
                            None => {
                                info!("Link channel closed (timeout or removal), closing connection");
                                break;
                            }
                        }
                    }
                    result = framed.next() => {
                        match result {
                            Some(Ok(line)) => {
                                if let Some(sid) = &remote_sid {
                                    S2S_BYTES_RECEIVED.with_label_values(&[sid.as_str()]).inc_by(line.len() as u64 + 2); // +2 for \r\n
                                }
                                let msg = match line.parse::<Message>() {
                                    Ok(m) => m,
                                    Err(e) => {
                                        tracing::warn!("Failed to parse inbound message: {}", e);
                                        continue;
                                    }
                                };

                                #[allow(clippy::collapsible_if)]
                                if let Some(sid) = &remote_sid {
                                    S2S_COMMANDS.with_label_values(&[sid.as_str(), msg.command.name()]).inc();
                                    if let Err(e) = handler.handle_message(msg, &manager, sid).await {
                                        tracing::error!("Protocol error from peer: {}", e);
                                        break;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                tracing::error!("Stream error: {}", e);
                                break;
                            }
                            None => {
                                info!("Connection closed by peer");
                                break;
                            }
                        }
                    }
                }
            }

            // Connection ended - handle netsplit
            if let Some(sid) = remote_sid {
                let rn = remote_name.as_deref().unwrap_or("unknown");
                info!(remote_sid = %sid.as_str(), "Peer disconnected, initiating netsplit cleanup");
                split::handle_netsplit(&matrix, &sid, &manager.local_name, rn).await;
            }
        });
    }

    /// Get a peer connection for a given server ID.
    pub fn get_peer_for_server(&self, sid: &ServerId) -> Option<LinkState> {
        self.links.get(sid).map(|l| l.clone())
    }

    // Legacy/Stub methods to satisfy existing code
    pub async fn register_peer(
        &self,
        sid: ServerId,
        name: String,
        hopcount: u32,
        info: String,
    ) -> mpsc::Receiver<Arc<Message>> {
        let (tx, rx) = mpsc::channel(1000);
        self.links.insert(
            sid.clone(),
            LinkState {
                tx,
                state: handshake::HandshakeState::Synced,
                name: name.clone(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
                connected_at: Instant::now(),
            },
        );
        self.topology.servers.insert(
            sid.clone(),
            ServerInfo {
                sid,
                name,
                info,
                hopcount,
                via: None, // Direct peer
            },
        );
        rx
    }

    pub async fn send_burst(
        &self,
        sid: &ServerId,
        _user_manager: &UserManager,
        _channel_manager: &ChannelManager,
    ) {
        info!("Sending burst to {}", sid.as_str());
    }

    pub async fn remove_peer(&self, sid: &ServerId) {
        self.links.remove(sid);
    }

    pub async fn broadcast(&self, _msg: Arc<Message>, _source: Option<&ServerId>) {
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
