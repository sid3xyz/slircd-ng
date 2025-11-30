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
use futures_util::SinkExt;
use slirc_proto::irc::IrcCodec;
use slirc_proto::transport::ZeroCopyTransport;
use slirc_proto::{irc_to_lower, Command, Message};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_util::codec::FramedWrite;
use tracing::{debug, info, instrument, warn};

// Rate limiter configuration constants (aligned with IRC standard: 5 messages per 2 seconds)
const RATE_LIMIT_RATE: f32 = 2.5;      // Messages per second (5 msg/2s)
const RATE_LIMIT_BURST: f32 = 5.0;     // Allow 5 message burst

/// IRC message encoding (UTF-8 is standard for modern IRC)
const IRC_ENCODING: &str = "utf-8";

/// Stream type enum to handle both plaintext and TLS connections.
enum ConnectionStream {
    Plaintext(TcpStream),
    Tls(TlsStream<TcpStream>),
}

/// A client connection handler.
pub struct Connection {
    uid: String,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    stream: ConnectionStream,
    db: Database,
    is_tls: bool,
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
            stream: ConnectionStream::Plaintext(stream),
            db,
            is_tls: false,
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
            stream: ConnectionStream::Tls(stream),
            db,
            is_tls: true,
        }
    }

    /// Helper to split the stream into read and write halves.
    fn split_stream(
        stream: ConnectionStream,
    ) -> (
        Box<dyn AsyncRead + Unpin + Send>,
        Box<dyn AsyncWrite + Unpin + Send>,
    ) {
        match stream {
            ConnectionStream::Plaintext(tcp) => {
                let (read, write) = tokio::io::split(tcp);
                (Box::new(read), Box::new(write))
            }
            ConnectionStream::Tls(tls) => {
                let (read, write) = tokio::io::split(tls);
                (Box::new(read), Box::new(write))
            }
        }
    }

    /// Run the connection read loop.
    #[instrument(skip(self), fields(uid = %self.uid, addr = %self.addr, tls = %self.is_tls), name = "connection")]
    pub async fn run(self) -> anyhow::Result<()> {
        info!(
            server = %self.matrix.server_info.name,
            tls = %self.is_tls,
            "Client connected"
        );

        // Split stream for concurrent read/write.
        // This enables true zero-copy reading during both handshake and main loop.
        let (read_half, write_half) = Self::split_stream(self.stream);
        let codec = IrcCodec::new(IRC_ENCODING)?;
        let mut reader = ZeroCopyTransport::new(read_half);
        let mut writer = FramedWrite::new(write_half, codec);

        // Channel for outgoing messages during handshake (drained synchronously)
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Message>(64);

        // Handshake state for this connection
        let mut handshake = HandshakeState::default();

        // Set +Z mode if TLS connection
        if self.is_tls {
            handshake.is_tls = true;
        }

        // Phase 1: Handshake using zero-copy reading
        // Read messages directly as MessageRef without intermediate allocations
        loop {
            match reader.next().await {
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
                        if let Err(e) = writer.send(response).await {
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
                    warn!(error = ?e, "Read error during handshake");
                    return Ok(());
                }
                None => {
                    info!("Client disconnected during handshake");
                    return Ok(());
                }
            }
        }

        // Phase 2: Unified Zero-Copy Loop
        // Reader and writer are already set up from handshake phase

        // Rate limiter for flood protection
        let mut rate_limiter = RateLimiter::new(RATE_LIMIT_RATE, RATE_LIMIT_BURST);
        
        // Penalty box: Track consecutive rate limit violations
        let mut flood_violations = 0u8;
        const MAX_FLOOD_VIOLATIONS: u8 = 3;  // Strike limit before disconnect

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
                // 'msg_ref' is borrowed from 'reader'. It exists ONLY inside this match block.
                result = reader.next() => {
                    match result {
                        Some(Ok(msg_ref)) => {
                            // Flood protection with penalty box
                            if !rate_limiter.check() {
                                flood_violations += 1;
                                warn!(uid = %self.uid, violations = flood_violations, "Rate limit exceeded");
                                
                                if flood_violations >= MAX_FLOOD_VIOLATIONS {
                                    // Strike limit reached - disconnect immediately
                                    warn!(uid = %self.uid, "Maximum flood violations reached - disconnecting");
                                    let _ = writer.send(Message::from(Command::ERROR("Excess Flood (Strike limit reached)".into()))).await;
                                    break;
                                } else {
                                    // Warning strike - throttle but don't disconnect yet
                                    let _ = writer.send(Message::from(Command::NOTICE(
                                        "*".to_string(),
                                        format!("*** Warning: Flooding detected ({}/{} strikes). Slow down or you will be disconnected.", 
                                                flood_violations, MAX_FLOOD_VIOLATIONS)
                                    ))).await;
                                    
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
                            warn!(error = ?e, "Read error");
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
                    if let Err(e) = writer.send(msg).await {
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
            self.matrix.record_whowas(&user.nick, &user.user, &user.host, &user.realname);
            
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

