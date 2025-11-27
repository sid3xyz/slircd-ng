//! Connection - Handles an individual client connection.
//!
//! Each Connection runs in its own Tokio task, handling the read loop
//! for a single IRC client.

use crate::state::Matrix;
use slirc_proto::Transport;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{debug, info, instrument, warn};

/// A client connection handler.
pub struct Connection {
    uid: String,
    transport: Transport,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
}

impl Connection {
    /// Create a new connection handler.
    pub fn new(uid: String, stream: TcpStream, addr: SocketAddr, matrix: Arc<Matrix>) -> Self {
        let transport = Transport::tcp(stream);
        Self {
            uid,
            transport,
            addr,
            matrix,
        }
    }

    /// Run the connection read loop.
    #[instrument(skip(self), fields(uid = %self.uid, addr = %self.addr), name = "connection")]
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!(
            server = %self.matrix.server_info.name,
            "Client connected"
        );

        // Phase 0: Just read and log messages
        loop {
            match self.transport.read_message().await {
                Ok(Some(msg)) => {
                    debug!(
                        raw = %msg,
                        "Received message"
                    );
                    // TODO: Phase 1 will dispatch to handlers here
                }
                Ok(None) => {
                    // Clean disconnect
                    info!("Client disconnected");
                    break;
                }
                Err(e) => {
                    warn!(error = ?e, "Read error");
                    break;
                }
            }
        }

        Ok(())
    }
}
