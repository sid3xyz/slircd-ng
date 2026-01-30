use crate::config::LinkBlock;

use crate::security::rate_limit::S2SRateLimitResult;
use crate::state::Matrix;
use crate::sync::{
    LinkState, SyncManager, burst,
    handshake::{HandshakeMachine, HandshakeState},
    split,
    stream::S2SStream,
    tls::DangerousNoVerifier,
};
use futures_util::{SinkExt, StreamExt};
use rustls_pemfile::{certs, pkcs8_private_keys};
use sha2::{Digest, Sha256};
use slirc_proto::sync::ServerId;
use slirc_proto::{Command, Message};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::info;

/// Upgrades a TCP stream to TLS for outbound connections.
pub async fn upgrade_to_tls(
    tcp_stream: TcpStream,
    hostname: &str,
    verify_cert: bool,
    cert_fingerprint: Option<&str>,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, Box<dyn std::error::Error + Send + Sync>> {
    use tokio_rustls::TlsConnector;

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

/// Starts the inbound S2S listener.
pub fn start_inbound_listener(
    manager: SyncManager,
    matrix: Arc<Matrix>,
    registry: Arc<crate::handlers::Registry>,
    db: crate::db::Database,
    s2s_tls: Option<crate::config::S2STlsConfig>,
    s2s_listen: Option<std::net::SocketAddr>,
) {
    // Start TLS S2S listener if configured
    if let Some(tls_config) = s2s_tls {
        let manager = manager.clone();
        let matrix = Arc::clone(&matrix);
        let registry = Arc::clone(&registry);
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = run_s2s_tls_listener(manager, matrix, registry, db, tls_config).await {
                tracing::error!(error = %e, "S2S TLS listener failed");
            }
        });
    }

    // Start plaintext S2S listener if configured (not recommended for production)
    if let Some(addr) = s2s_listen {
        let manager = manager.clone();
        let matrix = Arc::clone(&matrix);
        let registry = Arc::clone(&registry);
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = run_s2s_plaintext_listener(manager, matrix, registry, db, addr).await {
                tracing::error!(error = %e, "S2S plaintext listener failed");
            }
        });
    }
}

async fn run_s2s_tls_listener(
    manager: SyncManager,
    matrix: Arc<Matrix>,
    registry: Arc<crate::handlers::Registry>,
    db: crate::db::Database,
    config: crate::config::S2STlsConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(address = %config.address, "Starting S2S TLS listener");

    // Load certificate
    let cert_data = tokio::fs::read(&config.cert_path).await?;
    let cert_chain: Vec<CertificateDer<'static>> = certs(&mut Cursor::new(&cert_data))
        .filter_map(|r| r.ok())
        .collect();
    if cert_chain.is_empty() {
        return Err("No certificates found in cert file".into());
    }

    // Load private key
    let key_data = tokio::fs::read(&config.key_path).await?;
    let keys: Vec<PrivateKeyDer<'static>> = pkcs8_private_keys(&mut Cursor::new(&key_data))
        .filter_map(|r| r.ok())
        .map(PrivateKeyDer::Pkcs8)
        .collect();
    let key = keys.into_iter().next().ok_or("No private key found")?;

    // Build TLS config
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

    let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();

    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
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
                                    handle_inbound_connection(
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
            _ = shutdown_rx.recv() => {
                info!("S2S TLS listener stopping");
                break Ok(());
            }
        }
    }
}

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

    let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();

    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((tcp_stream, peer_addr)) => {
                        info!(peer = %peer_addr, "Inbound S2S plaintext connection");

                        let manager = manager.clone();
                        let matrix = Arc::clone(&matrix);
                        let registry = Arc::clone(&registry);
                        let db = db.clone();

                        tokio::spawn(async move {
                            let stream = S2SStream::Plain(tcp_stream);
                            handle_inbound_connection(
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
            _ = shutdown_rx.recv() => {
                info!("S2S plaintext listener stopping");
                break Ok(());
            }
        }
    }
}

async fn handle_inbound_connection(
    manager: SyncManager,
    matrix: Arc<Matrix>,
    registry: Arc<crate::handlers::Registry>,
    db: crate::db::Database,
    stream: S2SStream,
    remote_addr: std::net::SocketAddr,
    is_tls: bool,
) {
    let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
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
                        Message::from(Command::ERROR(format!("Loop detected: {} ({})", name, sid)))
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
                        if let Err(e) = framed.send(Message::from(cmd).to_string().trim_end()).await
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
        sasl_state: crate::handlers::SaslState::default(),
        sasl_buffer: String::new(),
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
                        crate::metrics::inc_s2s_bytes_sent(remote_sid_val.as_str(), s.len() as u64 + 2);
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
                        crate::metrics::inc_s2s_bytes_received(remote_sid_val.as_str(), line.len() as u64 + 2);
                        let msg = match line.parse::<Message>() {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!(peer = %remote_addr, error = %e, "Failed to parse message");
                                continue;
                            }
                        };

                        crate::metrics::inc_s2s_commands(remote_sid_val.as_str(), msg.command.name());

                        // Check S2S rate limit
                        match manager.rate_limiter.check_command(remote_sid_val.as_str()) {
                            S2SRateLimitResult::Allowed => {
                                // Continue processing
                            }
                            S2SRateLimitResult::Limited { violations } => {
                                crate::metrics::inc_s2s_rate_limited(remote_sid_val.as_str(), "limited");
                                tracing::warn!(
                                    sid = %remote_sid_val.as_str(),
                                    violations = violations,
                                    "S2S rate limit exceeded, dropping command"
                                );
                                continue;
                            }
                            S2SRateLimitResult::Disconnect { violations } => {
                                crate::metrics::inc_s2s_rate_limited(remote_sid_val.as_str(), "disconnected");
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
                        // DEBUG LOGGING
                        tracing::info!(raw = %raw_str, "Dispatching message to registry");

                        match slirc_proto::message::MessageRef::parse(&raw_str) {
                            Ok(msg_ref) => {
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
                            Err(e) => {
                                tracing::error!(peer = %remote_addr, raw = %raw_str, error = ?e, "MessageRef parse failed");
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
            _ = shutdown_rx.recv() => {
                info!(peer = %remote_addr, "Inbound S2S connection stopping due to shutdown");
                let _ = framed
                    .send(
                        Message::from(Command::ERROR("Server shutting down".into()))
                            .to_string()
                            .trim_end(),
                    )
                    .await;
                break;
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
    manager: SyncManager,
    matrix: Arc<Matrix>,
    registry: Arc<crate::handlers::Registry>,
    db: crate::db::Database,
    config: LinkBlock,
) {
    let manager = manager.clone();
    let matrix = matrix.clone();
    tokio::spawn(async move {
        let mut shutdown_rx = matrix.lifecycle_manager.shutdown_tx.subscribe();
        'reconnect_loop: loop {
            info!(hostname = %config.hostname, port = config.port, tls = config.tls, "Connecting to peer");

            // Establish TCP connection
            let tcp_stream =
                match TcpStream::connect(format!("{}:{}", config.hostname, config.port)).await {
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
                match upgrade_to_tls(
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
            let capab_cmd = Command::CAPAB(
                crate::sync::handshake::SUPPORTED_CAPABS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
            let server_cmd = Command::SERVER(
                manager.local_name.clone(),
                1,
                manager.local_id.as_str().to_string(),
                manager.local_desc.clone(),
            );
            let now_secs = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            {
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
                sasl_state: crate::handlers::SaslState::default(),
                sasl_buffer: String::new(),
            };

            // Reply channel for handler responses
            let (reply_tx, mut reply_rx) = mpsc::channel::<Arc<Message>>(100);
            let remote_addr = format!("{}:{}", config.hostname, config.port)
                .parse()
                .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 0)));

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!(peer = %config.hostname, "Outbound S2S connection stopping");
                        let _ = framed
                            .send(
                                Message::from(Command::ERROR("Server shutting down".into()))
                                    .to_string()
                                    .trim_end(),
                            )
                            .await;
                        break 'reconnect_loop;
                    }
                    msg = rx.recv() => {
                        match msg {
                            Some(m) => {
                                let s = m.as_ref().to_string();
                                crate::metrics::inc_s2s_bytes_sent(remote_sid_val.as_str(), s.len() as u64 + 2);
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
                                crate::metrics::inc_s2s_bytes_received(remote_sid_val.as_str(), line.len() as u64 + 2);
                                let msg = match line.parse::<Message>() {
                                    Ok(m) => m,
                                    Err(e) => {
                                        tracing::warn!("Failed to parse inbound message: {}", e);
                                        continue;
                                    }
                                };

                                crate::metrics::inc_s2s_commands(remote_sid_val.as_str(), msg.command.name());

                                // Check S2S rate limit before processing
                                match manager.rate_limiter.check_command(remote_sid_val.as_str()) {
                                    S2SRateLimitResult::Allowed => {
                                        // Continue to dispatch below
                                    }
                                    S2SRateLimitResult::Limited { violations } => {
                                        crate::metrics::inc_s2s_rate_limited(remote_sid_val.as_str(), "limited");
                                        tracing::warn!(
                                            sid = %remote_sid_val.as_str(),
                                            violations = violations,
                                            command = %msg.command.name(),
                                            "S2S rate limit exceeded, dropping command"
                                        );
                                        continue; // Drop the command but keep connection
                                    }
                                    S2SRateLimitResult::Disconnect { violations } => {
                                        crate::metrics::inc_s2s_rate_limited(remote_sid_val.as_str(), "disconnected");
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
