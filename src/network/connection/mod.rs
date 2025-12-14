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

mod error_handling;
mod lifecycle;

use lifecycle::{run_handshake_loop, run_event_loop};

use crate::db::Database;
use crate::handlers::Registry;
use crate::state::{Matrix, UnregisteredState};
use slirc_proto::transport::ZeroCopyTransportEnum;
use slirc_proto::Message;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tracing::{error, info, instrument};

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

    /// Run the connection lifecycle.
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

        // Channel for outgoing messages during handshake
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Message>(64);

        // Unregistered state for this connection
        let mut unreg_state = UnregisteredState::default();

        // Set +Z mode if TLS connection
        if is_tls {
            unreg_state.is_tls = true;
        }

        // Phase 1: Handshake
        if let Err(exit) = run_handshake_loop(
            &self.uid,
            &mut self.transport,
            &self.matrix,
            &self.registry,
            &self.db,
            self.addr,
            &mut unreg_state,
            &handshake_tx,
            &mut handshake_rx,
        )
        .await
        {
            exit.release_nick(&self.matrix);
            return Ok(());
        }

        // Transition: UnregisteredState -> RegisteredState
        // At this point, the user is registered and exists in Matrix.
        // Convert the state for Phase 2.
        let mut reg_state = match unreg_state.try_register() {
            Ok(state) => state,
            Err(_unreg) => {
                // This should not happen if handshake loop exited correctly,
                // but handle gracefully rather than panicking the connection task.
                error!(uid = %self.uid, "Registration state mismatch - closing connection");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Registration state mismatch",
                ).into());
            }
        };

        // Phase 2: Unified Event Loop
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Message>(32);
        self.matrix.register_sender(&self.uid, outgoing_tx.clone());

        let quit_message = run_event_loop(
            &self.uid,
            &mut self.transport,
            &self.matrix,
            &self.registry,
            &self.db,
            self.addr,
            &mut reg_state,
            &outgoing_tx,
            &mut outgoing_rx,
        )
        .await;

        // Canonical cleanup for normal disconnects.
        // If the user was already removed from the Matrix (KILL/enforcement/slow-consumer),
        // this will no-op.
        let quit_text = quit_message.unwrap_or_else(|| "Client Quit".to_string());
        let _ = self.matrix.disconnect_user(&self.uid, &quit_text).await;

        info!("Client disconnected");

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::error::{MessageParseError, ProtocolError};
    use slirc_proto::transport::TransportReadError;

    #[test]
    fn test_classify_message_too_long() {
        let err = TransportReadError::Protocol(ProtocolError::MessageTooLong {
            actual: 1024,
            limit: 512,
        });
        let action = classify_read_error(&err);
        // MessageTooLong is recoverable - returns InputTooLong
        assert!(matches!(action, ReadErrorAction::InputTooLong));
    }

    #[test]
    fn test_classify_tags_too_long() {
        let err = TransportReadError::Protocol(ProtocolError::TagsTooLong {
            actual: 8192,
            limit: 4096,
        });
        let action = classify_read_error(&err);
        // TagsTooLong is recoverable - returns InputTooLong
        assert!(matches!(action, ReadErrorAction::InputTooLong));
    }

    #[test]
    fn test_classify_illegal_control_char() {
        let err = TransportReadError::Protocol(ProtocolError::IllegalControlChar('\0'));
        let action = classify_read_error(&err);
        match action {
            ReadErrorAction::FatalProtocolError { error_msg } => {
                assert!(error_msg.contains("control character"));
            }
            _ => panic!("Expected FatalProtocolError"),
        }
    }

    #[test]
    fn test_classify_invalid_message() {
        let err = TransportReadError::Protocol(ProtocolError::InvalidMessage {
            string: "garbage".to_string(),
            cause: MessageParseError::InvalidCommand,
        });
        let action = classify_read_error(&err);
        match action {
            ReadErrorAction::FatalProtocolError { error_msg } => {
                assert!(error_msg.contains("Malformed"));
                assert!(error_msg.contains("garbage"));
            }
            _ => panic!("Expected FatalProtocolError"),
        }
    }

    #[test]
    fn test_classify_io_error() {
        let err = TransportReadError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));
        let action = classify_read_error(&err);
        assert!(matches!(action, ReadErrorAction::IoError));
    }
}
