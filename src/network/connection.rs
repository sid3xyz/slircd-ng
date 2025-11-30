//! Connection - Handles an individual client connection.
//!
//! Each Connection runs in its own Tokio task with the following architecture:
//!
//! ```text
//! Phase 1: Handshake (ZeroCopyTransport + FramedWrite, sequential)
//!    ↓
//! Phase 2: Unified Zero-Copy Loop (tokio::select!)
//!    ┌─────────────────────────────────────────────────────┐
//!    │              Unified Connection Task                │
//!    │                                                     │
//!    │  ┌─────────────────┐       ┌──────────────────┐    │
//!    │  │ ZeroCopyReader  │       │   FramedWrite    │    │
//!    │  └────────┬────────┘       └────────▲─────────┘    │
//!    │           │ (Borrow)                │              │
//!    │           ▼                         │              │
//!    │    tokio::select! ◄─────────────────┼──────────────┐
//!    │    │      │                         │              │
//!    │    │      ▼                         │              │
//!    │    │  [Handlers] ─────────▶ [Outgoing Queue]       │
//!    │    │  (Zero Alloc)                                 │
//!    │    └───────────────────────────────────────────────┘
//!    └─────────────────────────────────────────────────────┘
//! ```

use crate::db::Database;
use crate::handlers::{Context, HandshakeState, Registry};
use crate::network::limit::RateLimiter;
use crate::state::Matrix;
use slirc_proto::error::ProtocolError;
use slirc_proto::transport::{TransportReadError, ZeroCopyTransportEnum};
use slirc_proto::{Command, Message, irc_to_lower};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, info, instrument, warn};

/// Classification of transport read errors for appropriate handling.
enum ReadErrorAction {
    /// Recoverable protocol violation - send error message to client
    ProtocolViolation { error_msg: String },
    /// I/O error - connection is broken, just log and disconnect
    IoError,
}

/// Classify a transport read error into an actionable category.
fn classify_read_error(e: &TransportReadError) -> ReadErrorAction {
    match e {
        TransportReadError::Protocol(proto_err) => {
            let msg = match proto_err {
                ProtocolError::MessageTooLong { actual, limit } => {
                    format!("Input line too long ({actual} bytes, max {limit})")
                }
                ProtocolError::TagsTooLong { actual, limit } => {
                    format!("Message tags too long ({actual} bytes, max {limit})")
                }
                ProtocolError::IllegalControlChar(ch) => {
                    format!("Illegal control character: {ch:?}")
                }
                ProtocolError::InvalidMessage { string, cause } => {
                    format!("Malformed message: {cause} (input: {string:?})")
                }
                // Handle other variants that might be added in the future
                _ => format!("Protocol error: {proto_err}"),
            };
            ReadErrorAction::ProtocolViolation { error_msg: msg }
        }
        TransportReadError::Io(_) => ReadErrorAction::IoError,
        // Handle future variants gracefully
        _ => ReadErrorAction::IoError,
    }
}

/// A client connection handler.
pub struct Connection {
    uid: String,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    transport: ZeroCopyTransportEnum,
    db: Database,
}

impl Connection {
    /// Create a new plaintext connection handler.
    pub fn new_plaintext(
        uid: String,
        stream: TcpStream,
        addr: SocketAddr,
        matrix: Arc<Matrix>,
        registry: Arc<Registry>,
        db: Database,
    ) -> Self {
        Self {
            uid,
            addr,
            matrix,
            registry,
            transport: ZeroCopyTransportEnum::tcp(stream),
            db,
        }
    }

    /// Create a new TLS connection handler.
    pub fn new_tls(
        uid: String,
        stream: TlsStream<TcpStream>,
        addr: SocketAddr,
        matrix: Arc<Matrix>,
        registry: Arc<Registry>,
        db: Database,
    ) -> Self {
        Self {
            uid,
            addr,
            matrix,
            registry,
            transport: ZeroCopyTransportEnum::tls(stream),
            db,
        }
    }

    /// Create a new WebSocket connection handler.
    pub fn new_websocket(
        uid: String,
        stream: WebSocketStream<TcpStream>,
        addr: SocketAddr,
        matrix: Arc<Matrix>,
        registry: Arc<Registry>,
        db: Database,
    ) -> Self {
        Self {
            uid,
            addr,
            matrix,
            registry,
            transport: ZeroCopyTransportEnum::websocket(stream),
            db,
        }
    }

    /// Run the connection read loop.
    #[instrument(skip(self), fields(uid = %self.uid, addr = %self.addr), name = "connection")]
    pub async fn run(mut self) -> anyhow::Result<()> {
        // Detect connection type for logging
        let is_tls = matches!(
            self.transport,
            ZeroCopyTransportEnum::Tls(_)
                | ZeroCopyTransportEnum::ClientTls(_)
                | ZeroCopyTransportEnum::WebSocketTls(_)
        );
        let is_websocket = matches!(
            self.transport,
            ZeroCopyTransportEnum::WebSocket(_) | ZeroCopyTransportEnum::WebSocketTls(_)
        );

        info!(
            server = %self.matrix.server_info.name,
            tls = %is_tls,
            websocket = %is_websocket,
            "Client connected"
        );

        // Channel for outgoing messages during handshake (drained synchronously)
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Message>(64);

        // Handshake state for this connection
        let mut handshake = HandshakeState::default();

        // Set +Z mode if TLS connection
        if is_tls {
            handshake.is_tls = true;
        }

        // Phase 1: Handshake using zero-copy reading
        // Read messages directly as MessageRef without intermediate allocations
        loop {
            match self.transport.next().await {
                Some(Ok(msg_ref)) => {
                    debug!(raw = %msg_ref.raw.trim(), "Received message");

                    let mut ctx = Context {
                        uid: &self.uid,
                        matrix: &self.matrix,
                        sender: &handshake_tx,
                        handshake: &mut handshake,
                        db: &self.db,
                        remote_addr: self.addr,
                    };

                    if let Err(e) = self.registry.dispatch(&mut ctx, &msg_ref).await {
                        debug!(error = ?e, "Handler error");
                        if matches!(e, crate::handlers::HandlerError::NotRegistered) {
                            break;
                        }
                    }

                    // Drain and write queued responses synchronously
                    while let Ok(response) = handshake_rx.try_recv() {
                        if let Err(e) = self.transport.write_message(&response).await {
                            warn!(error = ?e, "Write error during handshake");
                            return Ok(());
                        }
                    }

                    // Check if handshake is complete
                    if handshake.registered {
                        break;
                    }
                }
                Some(Err(e)) => {
                    match classify_read_error(&e) {
                        ReadErrorAction::ProtocolViolation { error_msg } => {
                            warn!(error = %error_msg, "Protocol error during handshake");
                            // Send ERROR message before disconnecting
                            let error_reply = Message {
                                tags: None,
                                prefix: None,
                                command: Command::ERROR(error_msg),
                            };
                            let _ = self.transport.write_message(&error_reply).await;
                        }
                        ReadErrorAction::IoError => {
                            debug!(error = ?e, "I/O error during handshake");
                        }
                    }
                    return Ok(());
                }
                None => {
                    info!("Client disconnected during handshake");
                    return Ok(());
                }
            }
        }

        // Phase 2: Unified Zero-Copy Loop
        // Transport handles both reading and writing with unified API

        // Rate limiter for flood protection (configurable from config.toml)
        let limits = &self.matrix.config.limits;
        let mut rate_limiter = RateLimiter::new(limits.rate, limits.burst);

        // Penalty box: Track consecutive rate limit violations
        let mut flood_violations = 0u8;
        const MAX_FLOOD_VIOLATIONS: u8 = 3; // Strike limit before disconnect

        // Channel for outgoing messages (handlers queue responses here)
        // Also used for routing messages from other users (PRIVMSG, etc.)
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Message>(32);

        // Register sender with Matrix for message routing
        self.matrix.register_sender(&self.uid, outgoing_tx.clone());

        info!("Entering Unified Zero-Copy Loop");

        // Unified event loop using tokio::select!
        loop {
            tokio::select! {
                // BRANCH A: Network Input (Zero-Copy)
                // 'msg_ref' is borrowed from transport. It exists ONLY inside this match block.
                result = self.transport.next() => {
                    match result {
                        Some(Ok(msg_ref)) => {
                            // Flood protection with penalty box
                            if !rate_limiter.check() {
                                flood_violations += 1;
                                warn!(uid = %self.uid, violations = flood_violations, "Rate limit exceeded");

                                if flood_violations >= MAX_FLOOD_VIOLATIONS {
                                    // Strike limit reached - disconnect immediately
                                    warn!(uid = %self.uid, "Maximum flood violations reached - disconnecting");
                                    let error_msg = Message::from(Command::ERROR("Excess Flood (Strike limit reached)".into()));
                                    let _ = self.transport.write_message(&error_msg).await;
                                    break;
                                } else {
                                    // Warning strike - throttle but don't disconnect yet
                                    let notice = Message::from(Command::NOTICE(
                                        "*".to_string(),
                                        format!("*** Warning: Flooding detected ({}/{} strikes). Slow down or you will be disconnected.",
                                                flood_violations, MAX_FLOOD_VIOLATIONS)
                                    ));
                                    let _ = self.transport.write_message(&notice).await;

                                    // Apply penalty delay (exponential backoff)
                                    let penalty_ms = 500 * (flood_violations as u64);
                                    tokio::time::sleep(tokio::time::Duration::from_millis(penalty_ms)).await;
                                    continue;  // Skip processing this command
                                }
                            } else {
                                // Rate limit passed - reset violation counter
                                flood_violations = 0;
                            }

                            debug!(raw = ?msg_ref, "Received message (zero-copy)");

                            // Dispatch to handler
                            let mut ctx = Context {
                                uid: &self.uid,
                                matrix: &self.matrix,
                                sender: &outgoing_tx,
                                handshake: &mut handshake,
                                db: &self.db,
                                remote_addr: self.addr,
                            };

                            if let Err(e) = self.registry.dispatch(&mut ctx, &msg_ref).await {
                                debug!(error = ?e, "Handler error");
                                if matches!(e, crate::handlers::HandlerError::NotRegistered) {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            match classify_read_error(&e) {
                                ReadErrorAction::ProtocolViolation { error_msg } => {
                                    warn!(error = %error_msg, "Protocol error from client");
                                    // Send ERROR message before disconnecting
                                    let error_reply = Message {
                                        tags: None,
                                        prefix: None,
                                        command: Command::ERROR(error_msg),
                                    };
                                    let _ = self.transport.write_message(&error_reply).await;
                                }
                                ReadErrorAction::IoError => {
                                    debug!(error = ?e, "I/O error");
                                }
                            }
                            break;
                        }
                        None => {
                            info!("Client disconnected");
                            break;
                        }
                    }
                }

                // BRANCH B: Outgoing Messages
                // Handles responses queued by handlers AND messages routed from other users
                Some(msg) = outgoing_rx.recv() => {
                    if let Err(e) = self.transport.write_message(&msg).await {
                        warn!(error = ?e, "Write error");
                        break;
                    }
                }
            }
        }

        // Cleanup: record WHOWAS and remove user from all channels
        if let Some(user_ref) = self.matrix.users.get(&self.uid) {
            let user = user_ref.read().await;
            let channels: Vec<String> = user.channels.iter().cloned().collect();

            // Record WHOWAS entry before cleanup
            self.matrix
                .record_whowas(&user.nick, &user.user, &user.host, &user.realname);

            drop(user);

            for channel_lower in channels {
                if let Some(channel) = self.matrix.channels.get(&channel_lower) {
                    let mut channel = channel.write().await;
                    channel.remove_member(&self.uid);
                    // If channel is empty, it will be cleaned up eventually
                }
            }
        }
        self.matrix.users.remove(&self.uid);

        // Cleanup: remove nick from index
        if let Some(nick) = &handshake.nick {
            let nick_lower = irc_to_lower(nick);
            self.matrix.nicks.remove(&nick_lower);
            info!(nick = %nick, "Nick released");
        }

        // Unregister sender from Matrix
        self.matrix.unregister_sender(&self.uid);

        info!("Client disconnected");

        Ok(())
    }
}
