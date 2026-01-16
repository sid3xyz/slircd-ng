//! Sync Module - Server-to-Server Synchronization.
//!
//! This module manages the distributed state of the IRC network.
//! It handles server linking, handshake, and CRDT state replication.

pub mod burst;
pub mod handshake;
mod observer;
pub mod split;
pub mod stream;
#[cfg(test)]
mod tests;
pub mod topology;

use crate::metrics::{S2S_BYTES_RECEIVED, S2S_BYTES_SENT, S2S_COMMANDS, S2S_RATE_LIMITED};
use crate::security::rate_limit::S2SRateLimitResult;
use crate::state::Matrix;
use crate::sync::handshake::{HandshakeMachine, HandshakeState};
use crate::sync::stream::S2SStream;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use slirc_proto::sync::clock::ServerId;
use slirc_proto::{Command, Message};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::info;

use crate::config::LinkBlock;

// Re-export topology types
pub use topology::{ServerInfo, TopologyGraph};

use std::time::{Duration, Instant};

/// A certificate verifier that accepts all certificates.
/// DANGEROUS: Only use for testing or self-signed certificates.
#[derive(Debug)]
struct DangerousNoVerifier;

impl tokio_rustls::rustls::client::danger::ServerCertVerifier for DangerousNoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
        _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: tokio_rustls::rustls::pki_types::UnixTime,
    ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error>
    {
        // Accept all certificates without verification
        Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        // Support all common signature schemes
        vec![
            tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA256,
            tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA384,
            tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA512,
            tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            tokio_rustls::rustls::SignatureScheme::RSA_PSS_SHA256,
            tokio_rustls::rustls::SignatureScheme::RSA_PSS_SHA384,
            tokio_rustls::rustls::SignatureScheme::RSA_PSS_SHA512,
            tokio_rustls::rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Represents the state of a link to a peer server.
#[derive(Debug)]
pub struct LinkState {
    /// The channel to send messages to this peer.
    pub tx: mpsc::Sender<Arc<Message>>,
    /// The current handshake state.
    pub state: handshake::HandshakeState,
    /// The name of the peer server.
    pub name: String,
    /// Last time we received a PONG (or any data) from this peer.
    pub last_pong: Instant,
    /// Last time we sent a PING to this peer.
    pub last_ping: Instant,
    /// Time when the connection was established.
    pub connected_at: Instant,
}

impl Clone for LinkState {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            state: self.state.clone(),
            name: self.name.clone(),
            last_pong: self.last_pong,
            last_ping: self.last_ping,
            connected_at: self.connected_at,
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

    /// Upgrades a TCP stream to TLS for outbound connections.
    ///
    /// # Arguments
    /// * `tcp_stream` - The established TCP connection
    /// * `hostname` - The remote hostname (used for SNI and certificate verification)
    /// * `verify_cert` - Whether to verify the remote certificate
    /// * `cert_fingerprint` - Optional SHA-256 fingerprint for certificate pinning
    async fn upgrade_to_tls(
        tcp_stream: TcpStream,
        hostname: &str,
        verify_cert: bool,
        cert_fingerprint: Option<&str>,
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

        // Certificate fingerprint pinning
        if let Some(expected_fp) = cert_fingerprint {
            let (_, conn) = tls_stream.get_ref();
            if let Some(certs) = conn.peer_certificates()
                && let Some(cert) = certs.first()
            {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(cert.as_ref());
                let actual_fp = hasher.finalize();
                let actual_fp_hex = actual_fp
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(":");

                // Normalize expected fingerprint for comparison
                let expected_normalized = expected_fp.to_uppercase().replace([' ', '-'], ":");

                if actual_fp_hex != expected_normalized {
                    tracing::error!(
                        hostname = %hostname,
                        expected = %expected_normalized,
                        actual = %actual_fp_hex,
                        "Certificate fingerprint mismatch!"
                    );
                    return Err("Certificate fingerprint mismatch".into());
                }
                info!(hostname = %hostname, fingerprint = %actual_fp_hex, "Certificate fingerprint verified");
            }
        }

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

        // 3. Find next hop (must be a directly connected peer)
        if let Some(link) = self.get_next_hop(&target_sid) {
            let _ = link.tx.send(msg).await;
            crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                .with_label_values(&[self.local_id.as_str(), target_sid.as_str(), "success"])
                .inc();
            true
        } else {
            tracing::warn!(
                "No route to server {} (for user {})",
                target_sid.as_str(),
                target_uid
            );
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

    /// Starts the inbound S2S listener for accepting connections from remote servers.
    ///
    /// This spawns a background task that listens for incoming S2S connections.
    /// If `s2s_tls` is configured, the listener will accept TLS connections.
    /// If only `s2s_listen` is configured, it accepts plaintext connections.
    pub fn start_inbound_listener(
        &self,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        s2s_tls: Option<crate::config::S2STlsConfig>,
        s2s_listen: Option<std::net::SocketAddr>,
    ) {
        // Start TLS S2S listener if configured
        if let Some(tls_config) = s2s_tls {
            let manager = self.clone();
            let matrix = Arc::clone(&matrix);
            let registry = Arc::clone(&registry);
            let db = db.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    Self::run_s2s_tls_listener(manager, matrix, registry, db, tls_config).await
                {
                    tracing::error!(error = %e, "S2S TLS listener failed");
                }
            });
        }

        // Start plaintext S2S listener if configured (not recommended for production)
        if let Some(addr) = s2s_listen {
            let manager = self.clone();
            let matrix = Arc::clone(&matrix);
            let registry = Arc::clone(&registry);
            let db = db.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    Self::run_s2s_plaintext_listener(manager, matrix, registry, db, addr).await
                {
                    tracing::error!(error = %e, "S2S plaintext listener failed");
                }
            });
        }
    }

    /// Run the S2S TLS listener.
    async fn run_s2s_tls_listener(
        manager: SyncManager,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        config: crate::config::S2STlsConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use rustls_pemfile::{certs, pkcs8_private_keys};
        use std::io::Cursor;
        use tokio_rustls::TlsAcceptor;
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
        use tokio_rustls::rustls::server::WebPkiClientVerifier;
        use tokio_rustls::rustls::{RootCertStore, ServerConfig};

        info!(address = %config.address, "Starting S2S TLS listener");

        // Load certificate asynchronously (prevents executor stalls on slow storage)
        let cert_data = tokio::fs::read(&config.cert_path).await?;
        let cert_chain: Vec<CertificateDer<'static>> = certs(&mut Cursor::new(&cert_data))
            .filter_map(|r| r.ok())
            .collect();
        if cert_chain.is_empty() {
            return Err("No certificates found in cert file".into());
        }

        // Load private key asynchronously
        let key_data = tokio::fs::read(&config.key_path).await?;
        let keys: Vec<PrivateKeyDer<'static>> = pkcs8_private_keys(&mut Cursor::new(&key_data))
            .filter_map(|r| r.ok())
            .map(PrivateKeyDer::Pkcs8)
            .collect();
        let key = keys.into_iter().next().ok_or("No private key found")?;

        // Build TLS config with protocol version control
        use tokio_rustls::rustls::version::{TLS12, TLS13};

        let protocol_versions = if config.tls13_only {
            info!("S2S TLS configured with TLS 1.3 only mode");
            vec![&TLS13]
        } else {
            info!("S2S TLS configured with minimum version TLS 1.2");
            vec![&TLS13, &TLS12]
        };

        let builder = ServerConfig::builder_with_protocol_versions(&protocol_versions);

        let tls_config = if config.client_auth == crate::config::ClientAuth::None {
            builder
                .with_no_client_auth()
                .with_single_cert(cert_chain, key)?
        } else {
            // Load CA for client verification
            let ca_path = config
                .ca_path
                .as_ref()
                .ok_or("ca_path required for client_auth")?;
            let ca_data = tokio::fs::read(ca_path).await?;
            let ca_certs: Vec<CertificateDer<'static>> = certs(&mut Cursor::new(&ca_data))
                .filter_map(|r| r.ok())
                .collect();

            let mut root_store = RootCertStore::empty();
            for cert in ca_certs {
                root_store.add(cert)?;
            }

            let verifier_builder = WebPkiClientVerifier::builder(Arc::new(root_store));
            let verifier = if config.client_auth == crate::config::ClientAuth::Optional {
                verifier_builder.allow_unauthenticated().build()?
            } else {
                verifier_builder.build()?
            };

            builder
                .with_client_cert_verifier(verifier)
                .with_single_cert(cert_chain, key)?
        };

        let acceptor = TlsAcceptor::from(Arc::new(tls_config));
        let listener = tokio::net::TcpListener::bind(config.address).await?;
        info!(address = %config.address, "S2S TLS listener started");

        loop {
            match listener.accept().await {
                Ok((tcp_stream, addr)) => {
                    info!(peer = %addr, "Inbound S2S TLS connection");

                    let manager = manager.clone();
                    let matrix = Arc::clone(&matrix);
                    let registry = Arc::clone(&registry);
                    let db = db.clone();
                    let acceptor = acceptor.clone();

                    tokio::spawn(async move {
                        match acceptor.accept(tcp_stream).await {
                            Ok(tls_stream) => {
                                let stream = S2SStream::TlsServer(tls_stream);
                                Self::handle_inbound_connection(
                                    manager, matrix, registry, db, stream, addr,
                                    true, // is_tls
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::warn!(peer = %addr, error = %e, "S2S TLS handshake failed");
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to accept S2S TLS connection");
                }
            }
        }
    }

    /// Run the S2S plaintext listener (not recommended for production).
    async fn run_s2s_plaintext_listener(
        manager: SyncManager,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        addr: std::net::SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::warn!(address = %addr, "Starting S2S PLAINTEXT listener - NOT RECOMMENDED for production");

        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!(address = %addr, "S2S plaintext listener started");

        loop {
            match listener.accept().await {
                Ok((tcp_stream, peer_addr)) => {
                    info!(peer = %peer_addr, "Inbound S2S plaintext connection");

                    let manager = manager.clone();
                    let matrix = Arc::clone(&matrix);
                    let registry = Arc::clone(&registry);
                    let db = db.clone();

                    tokio::spawn(async move {
                        let stream = S2SStream::Plain(tcp_stream);
                        Self::handle_inbound_connection(
                            manager, matrix, registry, db, stream, peer_addr, false, // is_tls
                        )
                        .await;
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to accept S2S plaintext connection");
                }
            }
        }
    }

    /// Handle an inbound S2S connection after transport setup.
    async fn handle_inbound_connection(
        manager: SyncManager,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        stream: S2SStream,
        remote_addr: std::net::SocketAddr,
        is_tls: bool,
    ) {
        let mut framed = Framed::new(stream, LinesCodec::new());

        let mut machine = HandshakeMachine::new(
            manager.local_id.clone(),
            manager.local_name.clone(),
            manager.local_desc.clone(),
        );
        // For inbound connections, we wait for the remote to send PASS/CAPAB/SERVER first
        machine.transition(HandshakeState::InboundReceived);

        // Track remote server info
        let mut remote_sid: Option<ServerId> = None;
        let mut remote_name: Option<String> = None;
        let mut remote_info: Option<String> = None;
        let mut handshake_success = false;

        // Wait for handshake with timeout
        let handshake_timeout = Duration::from_secs(60);
        let handshake_start = Instant::now();

        while let Some(result) = framed.next().await {
            if handshake_start.elapsed() > handshake_timeout {
                tracing::warn!(peer = %remote_addr, "Inbound S2S handshake timeout");
                let _ = framed
                    .send(
                        Message::from(Command::ERROR("Handshake timeout".into()))
                            .to_string()
                            .trim_end(),
                    )
                    .await;
                return;
            }

            let line = match result {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(peer = %remote_addr, error = %e, "S2S read error during handshake");
                    return;
                }
            };

            let msg = match line.parse::<Message>() {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Loop detection for SERVER command
            if let Command::SERVER(name, _, sid, _) = &msg.command {
                let sid_obj = ServerId::new(sid.clone());
                if manager.topology.servers.contains_key(&sid_obj) {
                    tracing::error!(peer = %remote_addr, "Loop detected: {} ({})", name, sid);
                    let _ = framed
                        .send(
                            Message::from(Command::ERROR(format!(
                                "Loop detected: {} ({})",
                                name, sid
                            )))
                            .to_string()
                            .trim_end(),
                        )
                        .await;
                    return;
                }
            }

            match machine.step(msg.command, &manager.configured_links) {
                Ok(responses) => {
                    for resp in responses {
                        if let Err(e) = framed
                            .send(Message::from(resp).to_string().trim_end())
                            .await
                        {
                            tracing::error!(peer = %remote_addr, error = %e, "Failed to send handshake response");
                            return;
                        }
                    }

                    if machine.state == HandshakeState::Bursting {
                        info!(peer = %remote_addr, remote = ?machine.remote_name, "Inbound S2S handshake complete");

                        remote_sid = machine.remote_sid.clone();
                        remote_name = machine.remote_name.clone();
                        remote_info = machine.remote_info.clone();

                        // Generate and send burst
                        let burst = burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                        for cmd in burst {
                            if let Err(e) =
                                framed.send(Message::from(cmd).to_string().trim_end()).await
                            {
                                tracing::error!(peer = %remote_addr, error = %e, "Failed to send burst");
                                return;
                            }
                        }
                        handshake_success = true;
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(peer = %remote_addr, error = ?e, "Inbound S2S handshake error");
                    return;
                }
            }
        }

        // Verify handshake completed
        let remote_sid_val = match (handshake_success, remote_sid.clone()) {
            (true, Some(sid)) => sid,
            _ => {
                tracing::warn!(peer = %remote_addr, "Inbound S2S handshake incomplete");
                return;
            }
        };

        // Register link
        let (tx, mut rx) = mpsc::channel::<Arc<Message>>(100);
        manager.links.insert(
            remote_sid_val.clone(),
            LinkState {
                tx,
                state: HandshakeState::Synced,
                name: remote_name.clone().unwrap_or_default(),
                last_pong: Instant::now(),
                last_ping: Instant::now(),
                connected_at: Instant::now(),
            },
        );

        // Add to topology (direct peer's parent/uplink is the local server)
        manager.topology.add_server(
            remote_sid_val.clone(),
            remote_name.clone().unwrap_or_default(),
            remote_info.clone().unwrap_or_default(),
            1,
            Some(manager.local_id.clone()),
        );

        info!(
            peer = %remote_addr,
            sid = %remote_sid_val.as_str(),
            name = %remote_name.as_deref().unwrap_or("unknown"),
            tls = is_tls,
            "Inbound S2S link established"
        );

        // Create ServerState for Registry dispatch
        let mut server_state = crate::state::ServerState {
            name: remote_name.clone().unwrap_or_default(),
            sid: remote_sid_val.as_str().to_string(),
            info: String::new(),
            hopcount: 1,
            capabilities: std::collections::HashSet::new(),
            is_tls,
            active_batch: None,
            active_batch_ref: None,
            batch_routing: None,
        };

        // Reply channel for handler responses
        let (reply_tx, mut reply_rx) = mpsc::channel::<Arc<Message>>(100);

        // Main message loop
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(m) => {
                            let s = m.as_ref().to_string();
                            S2S_BYTES_SENT.with_label_values(&[remote_sid_val.as_str()]).inc_by(s.len() as u64 + 2);
                            if let Err(e) = framed.send(s.trim_end()).await {
                                tracing::error!(peer = %remote_addr, error = %e, "Failed to send to peer");
                                break;
                            }
                        }
                        None => {
                            info!(peer = %remote_addr, "Link channel closed");
                            break;
                        }
                    }
                }
                Some(reply) = reply_rx.recv() => {
                    let s = reply.as_ref().to_string();
                    if let Err(e) = framed.send(s.trim_end()).await {
                        tracing::error!(peer = %remote_addr, error = %e, "Failed to send reply");
                        break;
                    }
                }
                result = framed.next() => {
                    match result {
                        Some(Ok(line)) => {
                            S2S_BYTES_RECEIVED.with_label_values(&[remote_sid_val.as_str()]).inc_by(line.len() as u64 + 2);
                            let msg = match line.parse::<Message>() {
                                Ok(m) => m,
                                Err(e) => {
                                    tracing::warn!(peer = %remote_addr, error = %e, "Failed to parse message");
                                    continue;
                                }
                            };

                            S2S_COMMANDS.with_label_values(&[remote_sid_val.as_str(), msg.command.name()]).inc();

                            // Check S2S rate limit
                            match manager.rate_limiter.check_command(remote_sid_val.as_str()) {
                                S2SRateLimitResult::Allowed => {
                                    // Continue processing
                                }
                                S2SRateLimitResult::Limited { violations } => {
                                    S2S_RATE_LIMITED.with_label_values(&[remote_sid_val.as_str(), "limited"]).inc();
                                    tracing::warn!(
                                        sid = %remote_sid_val.as_str(),
                                        violations = violations,
                                        "S2S rate limit exceeded, dropping command"
                                    );
                                    continue;
                                }
                                S2SRateLimitResult::Disconnect { violations } => {
                                    S2S_RATE_LIMITED.with_label_values(&[remote_sid_val.as_str(), "disconnected"]).inc();
                                    tracing::error!(
                                        sid = %remote_sid_val.as_str(),
                                        violations = violations,
                                        "S2S rate limit threshold exceeded, disconnecting"
                                    );
                                    let _ = framed.send(
                                        Message::from(Command::ERROR(format!("Rate limit exceeded ({} violations)", violations)))
                                            .to_string()
                                            .trim_end(),
                                    ).await;
                                    break;
                                }
                            }

                            // Dispatch to registry
                            let raw_str = msg.to_string();
                            if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
                                let mut ctx = crate::handlers::Context {
                                    uid: "server",
                                    matrix: &matrix,
                                    sender: crate::handlers::ResponseMiddleware::Direct(&reply_tx),
                                    state: &mut server_state,
                                    db: &db,
                                    remote_addr,
                                    label: None,
                                    suppress_labeled_ack: false,
                                    active_batch_id: None,
                                    registry: &registry,
                                };

                                if let Err(e) = registry.dispatch_server(&mut ctx, &msg_ref).await {
                                    tracing::error!(peer = %remote_addr, error = ?e, "Protocol error");
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::error!(peer = %remote_addr, error = %e, "Stream error");
                            break;
                        }
                        None => {
                            info!(peer = %remote_addr, "Connection closed by peer");
                            break;
                        }
                    }
                }
            }
        }

        // Connection ended - handle netsplit
        let rn = remote_name.as_deref().unwrap_or("unknown");
        info!(sid = %remote_sid_val.as_str(), "Inbound peer disconnected, initiating netsplit cleanup");
        split::handle_netsplit(&matrix, &remote_sid_val, &manager.local_name, rn).await;

        // Clean up rate limiter state
        manager.rate_limiter.remove_peer(remote_sid_val.as_str());
    }

    /// Initiates an outbound connection.
    pub fn connect_to_peer(
        &self,
        matrix: Arc<Matrix>,
        registry: Arc<crate::handlers::Registry>,
        db: crate::db::Database,
        config: LinkBlock,
    ) {
        let manager = self.clone();
        let matrix = matrix.clone();
        tokio::spawn(async move {
            loop {
                info!(hostname = %config.hostname, port = config.port, tls = config.tls, "Connecting to peer");

                // Establish TCP connection
                let tcp_stream = match TcpStream::connect(format!(
                    "{}:{}",
                    config.hostname, config.port
                ))
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(
                            "Failed to connect to {}: {}. Retrying in 5s...",
                            config.hostname,
                            e
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                // Upgrade to TLS if configured
                let stream: S2SStream = if config.tls {
                    match Self::upgrade_to_tls(
                        tcp_stream,
                        &config.hostname,
                        config.verify_cert,
                        config.cert_fingerprint.as_deref(),
                    )
                    .await
                    {
                        Ok(tls_stream) => S2SStream::TlsClient(tls_stream),
                        Err(e) => {
                            tracing::error!(
                                "TLS handshake failed with {}: {}. Retrying in 5s...",
                                config.hostname,
                                e
                            );
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
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
                let mut remote_info: Option<String> = None;

                // Send initial PASS, CAPAB, SERVER, SVINFO
                let pass_cmd = Command::PassTs6 {
                    password: config.password.clone(),
                    sid: manager.local_id.as_str().to_string(),
                };
                let capab_cmd = Command::CAPAB(vec![
                    "QS".to_string(),
                    "ENCAP".to_string(),
                    "EX".to_string(),
                    "IE".to_string(),
                    "UNKLN".to_string(),
                    "KLN".to_string(),
                    "GLN".to_string(),
                    "HOPS".to_string(),
                ]);
                let server_cmd = Command::SERVER(
                    manager.local_name.clone(),
                    1,
                    manager.local_id.as_str().to_string(),
                    manager.local_desc.clone(),
                );
                let now_secs =
                    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                        Ok(d) => d.as_secs(),
                        Err(e) => {
                            tracing::error!("System clock before UNIX_EPOCH: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    };
                let svinfo_cmd = Command::SVINFO(6, 6, 0, now_secs);

                if let Err(e) = framed
                    .send(Message::from(pass_cmd).to_string().trim_end())
                    .await
                {
                    tracing::error!("Failed to send PASS: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                if let Err(e) = framed
                    .send(Message::from(capab_cmd).to_string().trim_end())
                    .await
                {
                    tracing::error!("Failed to send CAPAB: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                if let Err(e) = framed
                    .send(Message::from(server_cmd).to_string().trim_end())
                    .await
                {
                    tracing::error!("Failed to send SERVER: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                if let Err(e) = framed
                    .send(Message::from(svinfo_cmd).to_string().trim_end())
                    .await
                {
                    tracing::error!("Failed to send SVINFO: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                let links = vec![config.clone()];
                let mut handshake_success = false;

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
                                    .to_string()
                                    .trim_end(),
                                )
                                .await;
                            break;
                        }
                    }

                    match machine.step(msg.command, &links) {
                        Ok(responses) => {
                            for resp in responses {
                                if let Err(e) = framed
                                    .send(Message::from(resp).to_string().trim_end())
                                    .await
                                {
                                    tracing::error!("Failed to send response: {}", e);
                                    break;
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
                                remote_info = machine.remote_info.clone();

                                // Generate Burst
                                let burst =
                                    burst::generate_burst(&matrix, manager.local_id.as_str()).await;
                                for cmd in burst {
                                    if let Err(e) =
                                        framed.send(Message::from(cmd).to_string().trim_end()).await
                                    {
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
                                        break;
                                    }
                                }
                                handshake_success = true;
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Handshake error with {}: {:?}", config.hostname, e);
                            break;
                        }
                    }
                }

                // Both handshake success AND remote_sid must be present to proceed
                let remote_sid_val = match (handshake_success, remote_sid.clone()) {
                    (true, Some(sid)) => sid,
                    _ => {
                        tracing::info!("Handshake failed or incomplete. Retrying in 5s...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                if let Some(expected_sid) = config.sid.as_deref()
                    && expected_sid != remote_sid_val.as_str()
                {
                    tracing::error!(
                        link = %config.name,
                        expected_sid = %expected_sid,
                        got_sid = %remote_sid_val.as_str(),
                        "Outbound S2S link SID mismatch; refusing connection"
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                // Register link
                let (tx, mut rx) = mpsc::channel::<Arc<Message>>(100);
                manager.links.insert(
                    remote_sid_val.clone(),
                    LinkState {
                        tx,
                        state: handshake::HandshakeState::Synced,
                        name: remote_name.clone().unwrap_or_default(),
                        last_pong: Instant::now(),
                        last_ping: Instant::now(),
                        connected_at: Instant::now(),
                    },
                );

                // Add to topology (direct peer's parent/uplink is the local server)
                manager.topology.add_server(
                    remote_sid_val.clone(),
                    remote_name.clone().unwrap_or_default(),
                    remote_info.clone().unwrap_or_default(),
                    1,
                    Some(manager.local_id.clone()),
                );

                // Create ServerState for Registry dispatch
                let mut server_state = crate::state::ServerState {
                    name: remote_name.clone().unwrap_or_default(),
                    sid: remote_sid_val.as_str().to_string(),
                    info: String::new(),
                    hopcount: 1,
                    capabilities: std::collections::HashSet::new(),
                    is_tls: config.tls,
                    active_batch: None,
                    active_batch_ref: None,
                    batch_routing: None,
                };

                // Reply channel for handler responses
                let (reply_tx, mut reply_rx) = mpsc::channel::<Arc<Message>>(100);
                let remote_addr = format!("{}:{}", config.hostname, config.port)
                    .parse()
                    .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 0)));

                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            match msg {
                                Some(m) => {
                                    let s = m.as_ref().to_string();
                                    S2S_BYTES_SENT.with_label_values(&[remote_sid_val.as_str()]).inc_by(s.len() as u64 + 2);
                                    if let Err(e) = framed.send(s.trim_end()).await {
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
                        Some(reply) = reply_rx.recv() => {
                            let s = reply.as_ref().to_string();
                            if let Err(e) = framed.send(s.trim_end()).await {
                                tracing::error!("Failed to send reply to peer: {}", e);
                                break;
                            }
                        }
                        result = framed.next() => {
                            match result {
                                Some(Ok(line)) => {
                                    S2S_BYTES_RECEIVED.with_label_values(&[remote_sid_val.as_str()]).inc_by(line.len() as u64 + 2);
                                    let msg = match line.parse::<Message>() {
                                        Ok(m) => m,
                                        Err(e) => {
                                            tracing::warn!("Failed to parse inbound message: {}", e);
                                            continue;
                                        }
                                    };

                                    S2S_COMMANDS.with_label_values(&[remote_sid_val.as_str(), msg.command.name()]).inc();

                                    // Check S2S rate limit before processing
                                    match manager.rate_limiter.check_command(remote_sid_val.as_str()) {
                                        S2SRateLimitResult::Allowed => {
                                            // Continue to dispatch below
                                        }
                                        S2SRateLimitResult::Limited { violations } => {
                                            S2S_RATE_LIMITED.with_label_values(&[remote_sid_val.as_str(), "limited"]).inc();
                                            tracing::warn!(
                                                sid = %remote_sid_val.as_str(),
                                                violations = violations,
                                                command = %msg.command.name(),
                                                "S2S rate limit exceeded, dropping command"
                                            );
                                            continue; // Drop the command but keep connection
                                        }
                                        S2SRateLimitResult::Disconnect { violations } => {
                                            S2S_RATE_LIMITED.with_label_values(&[remote_sid_val.as_str(), "disconnected"]).inc();
                                            tracing::error!(
                                                sid = %remote_sid_val.as_str(),
                                                violations = violations,
                                                "S2S rate limit threshold exceeded, disconnecting peer"
                                            );
                                            // Send ERROR before disconnecting
                                            let error_msg = Command::ERROR(format!(
                                                "Rate limit exceeded ({} violations)",
                                                violations
                                            ));
                                            let _ = framed.send(Message::from(error_msg).to_string().trim_end()).await;
                                            break; // Disconnect
                                        }
                                    }

                                    // Parse into MessageRef for Registry dispatch
                                    let raw_str = msg.to_string();
                                    if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
                                        let mut ctx = crate::handlers::Context {
                                            uid: "server",
                                            matrix: &matrix,
                                            sender: crate::handlers::ResponseMiddleware::Direct(&reply_tx),
                                            state: &mut server_state,
                                            db: &db,
                                            remote_addr,
                                            label: None,
                                            suppress_labeled_ack: false,
                                            active_batch_id: None,
                                            registry: &registry,
                                        };

                                        if let Err(e) = registry.dispatch_server(&mut ctx, &msg_ref).await {
                                            tracing::error!("Protocol error from peer: {:?}", e);
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
                let rn = remote_name.as_deref().unwrap_or("unknown");
                info!(remote_sid = %remote_sid_val.as_str(), "Peer disconnected, initiating netsplit cleanup");
                split::handle_netsplit(&matrix, &remote_sid_val, &manager.local_name, rn).await;

                // Clean up rate limiter state for this peer
                manager.rate_limiter.remove_peer(remote_sid_val.as_str());

                // Retry after disconnect
                info!("Reconnecting to {} in 5s...", config.hostname);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
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
        let commands = burst::generate_burst(matrix, self.local_id.as_str()).await;

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
