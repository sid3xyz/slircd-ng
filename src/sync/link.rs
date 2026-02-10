use crate::sync::handshake;
use slirc_proto::Message;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;
use tokio::sync::mpsc;

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
    /// Total bytes sent to this peer.
    pub bytes_sent: Arc<AtomicU64>,
    /// Total bytes received from this peer.
    pub bytes_recv: Arc<AtomicU64>,
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
            bytes_sent: self.bytes_sent.clone(),
            bytes_recv: self.bytes_recv.clone(),
        }
    }
}
