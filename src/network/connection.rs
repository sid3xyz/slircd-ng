//! Connection - Handles an individual client connection.
//!
//! Each Connection runs in its own Tokio task with the following architecture:
//!
//! ```text
//! Phase 1: Handshake (Transport - owned Messages, sequential)
//!    ↓
//! Phase 2: Upgrade & Split
//!    ↓
//! Phase 3: Streaming (concurrent tasks)
//!    ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//!    │ Reader Task │────▶│ Actor Loop  │────▶│ Writer Task │
//!    │ (ZeroCopy)  │     │ (Handlers)  │     │ (Framed)    │
//!    └─────────────┘     └─────────────┘     └─────────────┘
//! ```

use crate::db::Database;
use crate::handlers::{Context, HandshakeState, Registry};
use crate::state::Matrix;
use futures_util::SinkExt;
use slirc_proto::transport::ZeroCopyTransport;
use slirc_proto::{irc_to_lower, Message, Transport};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::FramedWrite;
use tracing::{debug, error, info, instrument, warn};

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
        let mut transport = Transport::tcp(self.stream);

        // Channel for outgoing messages during handshake (drained synchronously)
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Message>(64);

        // Handshake state for this connection
        let mut handshake = HandshakeState::default();

        // Run handshake loop until registered
        loop {
            match transport.read_message().await {
                Ok(Some(msg)) => {
                    debug!(raw = %msg, "Received message");

                    let mut ctx = Context {
                        uid: &self.uid,
                        matrix: &self.matrix,
                        sender: &handshake_tx,
                        handshake: &mut handshake,
                        db: &self.db,
                    };

                    if let Err(e) = self.registry.dispatch(&mut ctx, &msg).await {
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

        // Phase 2: Upgrade & Split for concurrent read/write
        let parts = match transport.into_parts() {
            Ok(p) => p,
            Err(e) => {
                error!(error = ?e, "Failed to split transport");
                return Err(anyhow::anyhow!("Transport split failed"));
            }
        };
        let (read_half, write_half) = parts.split();

        // Phase 3: Concurrent Reader/Writer/Actor architecture
        //
        // Channels:
        //   incoming_tx (Reader) → incoming_rx (Actor): parsed messages from client
        //   outgoing_tx (Actor) → outgoing_rx (Writer): messages to send to client
        //
        // Ownership:
        //   - Reader task SOLELY owns incoming_tx (EOF detection relies on this!)
        //   - Writer task owns outgoing_rx
        //   - Actor loop owns incoming_rx and outgoing_tx

        let (incoming_tx, mut incoming_rx) = mpsc::channel::<Message>(32);
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<Message>(32);

        // Register sender with Matrix for message routing
        self.matrix.register_sender(&self.uid, outgoing_tx.clone());

        // --- WRITER TASK ---
        let write_handle = tokio::spawn(async move {
            let mut writer = FramedWrite::new(write_half.half, write_half.codec);
            let mut rx = outgoing_rx;

            while let Some(msg) = rx.recv().await {
                if let Err(e) = writer.send(msg).await {
                    warn!(error = ?e, "Write error");
                    break;
                }
            }
        });

        // --- READER TASK ---
        // Reader task is the SOLE owner of incoming_tx.
        // When client disconnects, reader ends, incoming_tx drops,
        // and incoming_rx.recv() returns None → actor loop exits cleanly.
        let read_handle = tokio::spawn(async move {
            let mut reader = ZeroCopyTransport::with_buffer(read_half.half, read_half.read_buf);

            while let Some(result) = reader.next().await {
                match result {
                    Ok(msg_ref) => {
                        debug!(raw = ?msg_ref, "Received message (zero-copy)");
                        // Convert to owned for sending across channel
                        if incoming_tx.send(msg_ref.to_owned()).await.is_err() {
                            break; // Actor died, stop reading
                        }
                    }
                    Err(e) => {
                        warn!(error = ?e, "Read error");
                        break;
                    }
                }
            }
            // incoming_tx drops here → signals actor that connection closed
        });

        // --- ACTOR LOOP ---
        // Process incoming messages and dispatch to handlers
        while let Some(msg) = incoming_rx.recv().await {
            let mut ctx = Context {
                uid: &self.uid,
                matrix: &self.matrix,
                sender: &outgoing_tx,
                handshake: &mut handshake,
                db: &self.db,
            };

            if let Err(e) = self.registry.dispatch(&mut ctx, &msg).await {
                debug!(error = ?e, "Handler error");
                if matches!(e, crate::handlers::HandlerError::NotRegistered) {
                    break;
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

        // Shutdown: drop outgoing_tx to signal writer, wait for tasks
        drop(outgoing_tx);
        let _ = read_handle.await;
        let _ = write_handle.await;

        Ok(())
    }
}
