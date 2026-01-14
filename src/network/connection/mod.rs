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

mod autoreplay;
mod context;
mod error_handling;
mod event_loop;
mod handshake;
mod helpers;
mod server_loop;

use context::{ConnectionContext, LifecycleChannels};
use event_loop::run_event_loop;
use handshake::{HandshakeSuccess, run_handshake_loop};
use server_loop::run_server_loop;

use crate::db::Database;
use crate::handlers::Registry;
use crate::state::{InitiatorData, Matrix, UnregisteredState};
use slirc_crdt::clock::ServerId;
use slirc_proto::Message;
use slirc_proto::transport::ZeroCopyTransportEnum;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
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
    /// TLS acceptor for STARTTLS upgrade (only available on plaintext connections).
    starttls_acceptor: Option<TlsAcceptor>,
    /// Data for initiating a server connection.
    initiator_data: Option<InitiatorData>,
}

impl Connection {
    /// Create a new plaintext connection handler.
    ///
    /// If `starttls_acceptor` is provided, the connection can be upgraded to TLS
    /// via the STARTTLS command before registration completes.
    pub fn new_plaintext(
        uid: String,
        stream: TcpStream,
        addr: SocketAddr,
        matrix: Arc<Matrix>,
        registry: Arc<Registry>,
        db: Database,
        starttls_acceptor: Option<TlsAcceptor>,
    ) -> Self {
        let mut transport = ZeroCopyTransportEnum::tcp(stream);
        // Enforce IRCv3 line length limit (8191 bytes) to support message-tags.
        // RFC 1459/2812 specified 512, but modern IRC requires more for tags.
        transport.set_max_line_len(slirc_proto::transport::MAX_IRC_LINE_LEN);

        Self {
            uid,
            addr,
            matrix,
            registry,
            transport,
            db,
            starttls_acceptor,
            initiator_data: None,
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
        let mut transport = ZeroCopyTransportEnum::tls(stream);
        // Enforce IRCv3 line length limit (8191 bytes) to support message-tags.
        transport.set_max_line_len(slirc_proto::transport::MAX_IRC_LINE_LEN);

        Self {
            uid,
            addr,
            matrix,
            registry,
            transport,
            db,
            starttls_acceptor: None, // Already TLS, no STARTTLS needed
            initiator_data: None,
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
            starttls_acceptor: None, // WebSocket doesn't support STARTTLS
            initiator_data: None,
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
        let (handshake_tx, mut handshake_rx) = mpsc::channel::<Arc<Message>>(64);

        // Unregistered state for this connection
        let mut unreg_state = UnregisteredState {
            initiator_data: self.initiator_data,
            ..Default::default()
        };

        // Set +Z mode if TLS connection
        if is_tls {
            unreg_state.is_tls = true;
        }

        // Phase 1: Handshake
        let success = match run_handshake_loop(
            ConnectionContext {
                uid: &self.uid,
                transport: &mut self.transport,
                matrix: &self.matrix,
                registry: &self.registry,
                db: &self.db,
                addr: self.addr,
                starttls_acceptor: self.starttls_acceptor.as_ref(),
            },
            LifecycleChannels {
                tx: &handshake_tx,
                rx: &mut handshake_rx,
            },
            &mut unreg_state,
        )
        .await
        {
            Ok(s) => s,
            Err(exit) => {
                exit.release_nick(&self.matrix);
                return Ok(());
            }
        };

        match success {
            HandshakeSuccess::User => {
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
                        )
                        .into());
                    }
                };

                // Phase 2: Unified Event Loop
                let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Arc<Message>>(32);
                self.matrix.register_sender(&self.uid, outgoing_tx.clone());

                let quit_message = run_event_loop(
                    ConnectionContext {
                        uid: &self.uid,
                        transport: &mut self.transport,
                        matrix: &self.matrix,
                        registry: &self.registry,
                        db: &self.db,
                        addr: self.addr,
                        starttls_acceptor: None, // STARTTLS not available after registration
                    },
                    LifecycleChannels {
                        tx: &outgoing_tx,
                        rx: &mut outgoing_rx,
                    },
                    &mut reg_state,
                )
                .await;

                // Canonical cleanup for normal disconnects.
                // If the user was already removed from the Matrix (KILL/enforcement/slow-consumer),
                // this will no-op.
                let quit_text = quit_message.unwrap_or_else(|| "Client Quit".to_string());
                let _ = self.matrix.disconnect_user(&self.uid, &quit_text).await;

                info!("Client disconnected");
            }
            HandshakeSuccess::Server => {
                // Transition: UnregisteredState -> ServerState
                // Handshake loop already sent credentials in handle_inbound_step.
                // We do NOT need to send them again.
                let send_handshake = false;

                let server_state = match unreg_state.try_register_server() {
                    Ok(state) => state,
                    Err(_) => {
                        error!(uid = %self.uid, "Server registration state mismatch");
                        return Ok(());
                    }
                };

                info!(
                    name = %server_state.name,
                    sid = %server_state.sid,
                    "Server registered - starting sync loop"
                );

                // Add to topology (direct peer's parent/uplink is the local server)
                self.matrix.sync_manager.topology.add_server(
                    ServerId::new(server_state.sid.clone()),
                    server_state.name.clone(),
                    server_state.info.clone(),
                    server_state.hopcount,
                    Some(self.matrix.sync_manager.local_id.clone()),
                );

                // Phase 2: Server Sync Loop
                if let Err(e) = run_server_loop(
                    ConnectionContext {
                        uid: &self.uid,
                        transport: &mut self.transport,
                        matrix: &self.matrix,
                        registry: &self.registry,
                        db: &self.db,
                        addr: self.addr,
                        starttls_acceptor: None,
                    },
                    server_state,
                    send_handshake,
                )
                .await
                {
                    error!(uid = %self.uid, error = ?e, "Server sync loop error");
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::error_handling::{ReadErrorAction, classify_read_error};
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
