//! Connection - Handles an individual client connection.
//!
//! Each Connection runs in its own Tokio task with the following architecture:
//!
//! ```text
//! Phase 1: Handshake (Transport - owned Messages, sequential)
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
use slirc_proto::transport::ZeroCopyTransport;
use slirc_proto::{irc_to_lower, Command, Message, MessageRef, Transport};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::FramedWrite;
use tracing::{debug, error, info, instrument, warn};

// Rate limiter configuration constants
const RATE_LIMIT_RATE: f32 = 10.0;     // Messages per second
const RATE_LIMIT_BURST: f32 = 20.0;    // Burst capacity

/// A client connection handler.
pub struct Connection {
    uid: String,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    stream: TcpStream,
    db: Database,
}

impl Connection {
    /// Create a new connection handler.
    pub fn new(
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
            stream,
            db,
        }
    }

    /// Run the connection read loop.
    #[instrument(skip(self), fields(uid = %self.uid, addr = %self.addr), name = "connection")]
    pub async fn run(self) -> anyhow::Result<()> {
        info!(
            server = %self.matrix.server_info.name,
            "Client connected"
        );

        // Phase 1: Handshake using Transport (owned Message reads/writes)
        // Per design: Keep as-is since network latency dominates this cold path.
        let mut transport = Transport::tcp(self.stream)?;

        // Channel for outgoing messages during handshake (drained synchronously)
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Message>(64);

        // Handshake state for this connection
        let mut handshake = HandshakeState::default();

        // Run handshake loop until registered
        loop {
            match transport.read_message().await {
                Ok(Some(msg)) => {
                    debug!(raw = %msg, "Received message");

                    // Design decision: Handshake uses owned Message but we convert to MessageRef
                    // for handlers. This is acceptable because:
                    // 1. Handshake is a cold path (network latency dominates)
                    // 2. Only ~3-5 messages during handshake (CAP, NICK, USER)
                    // 3. Keeps handler API uniform across handshake and main loop
                    let raw_str = msg.to_string();
                    let msg_ref = match MessageRef::parse(&raw_str) {
                        Ok(r) => r,
                        Err(e) => {
                            debug!(error = ?e, "Failed to parse message for handler");
                            continue;
                        }
                    };

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
                        if let Err(e) = transport.write_message(&response).await {
                            warn!(error = ?e, "Write error during handshake");
                            return Ok(());
                        }
                    }

                    // Check if handshake is complete
                    if handshake.registered {
                        break;
                    }
                }
                Ok(None) => {
                    info!("Client disconnected during handshake");
                    return Ok(());
                }
                Err(e) => {
                    warn!(error = ?e, "Read error during handshake");
                    return Ok(());
                }
            }
        }

        // Phase 2: Unified Zero-Copy Loop
        // Split transport for concurrent read/write
        let parts = match transport.into_parts() {
            Ok(p) => p,
            Err(e) => {
                error!(error = ?e, "Failed to split transport");
                return Err(anyhow::anyhow!("Transport split failed"));
            }
        };
        let (read_half, write_half) = parts.split();

        // Setup reader and writer
        let mut reader = ZeroCopyTransport::with_buffer(read_half.half, read_half.read_buf);
        let mut writer = FramedWrite::new(write_half.half, write_half.codec);

        // Rate limiter for flood protection
        let mut rate_limiter = RateLimiter::new(RATE_LIMIT_RATE, RATE_LIMIT_BURST);

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
                            // Flood protection
                            if !rate_limiter.check() {
                                warn!(uid = %self.uid, "Rate limit exceeded");
                                let _ = writer.send(Message::from(Command::ERROR("Excess Flood".into()))).await;
                                break;
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

        // Cleanup: remove user from all channels
        if let Some(user) = self.matrix.users.get(&self.uid) {
            let user = user.read().await;
            let channels: Vec<String> = user.channels.iter().cloned().collect();
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
