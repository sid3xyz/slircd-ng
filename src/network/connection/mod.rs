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

mod batch_state;
mod error_handling;

use batch_state::to_base36;
use error_handling::{ReadErrorAction, classify_read_error, handler_error_to_reply};

use crate::db::Database;
use crate::handlers::{
    Context, Registry, ResponseMiddleware, cleanup_monitors, labeled_ack,
    notify_monitors_offline, process_batch_message, send_welcome_burst, with_label,
};
use crate::state::{Matrix, UnregisteredState};
use slirc_proto::transport::ZeroCopyTransportEnum;
use slirc_proto::{BatchSubCommand, Command, Message, Prefix, Response, irc_to_lower};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, error, info, instrument, warn};

/// A client connection handler.
pub struct Connection {
    uid: String,
    addr: SocketAddr,
    matrix: Arc<Matrix>,
    registry: Arc<Registry>,
    transport: ZeroCopyTransportEnum,
    db: Database,
    batch_counter: u32,
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
            batch_counter: 0,
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
            batch_counter: 0,
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
            batch_counter: 0,
        }
    }

    /// Generate a per-connection batch identifier (base36, sequential, wraps on overflow).
    fn next_batch_id(&mut self) -> String {
        self.batch_counter = self.batch_counter.wrapping_add(1);
        to_base36(self.batch_counter)
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

        // Unregistered state for this connection (Phase 3 typestate)
        let mut unreg_state = UnregisteredState::default();

        // Set +Z mode if TLS connection
        if is_tls {
            unreg_state.is_tls = true;
        }

        // Phase 1: Handshake using zero-copy reading
        // Read messages directly as MessageRef without intermediate allocations
        loop {
            match self.transport.next().await {
                Some(Ok(msg_ref)) => {
                    debug!(raw = %msg_ref.raw.trim(), "Received message");

                    // Extract label tag for labeled-response (IRCv3)
                    let label = if unreg_state.capabilities.contains("labeled-response") {
                        msg_ref
                            .tags_iter()
                            .find(|(k, _)| *k == "label")
                            .map(|(_, v)| v.to_string())
                    } else {
                        None
                    };

                    let mut ctx = Context {
                        uid: &self.uid,
                        matrix: &self.matrix,
                        sender: ResponseMiddleware::Direct(&handshake_tx),
                        state: &mut unreg_state,
                        db: &self.db,
                        remote_addr: self.addr,
                        label,
                        suppress_labeled_ack: false,
                        registry: &self.registry,
                    };

                    if let Err(e) = self.registry.dispatch_pre_reg(&mut ctx, &msg_ref).await {
                        debug!(error = ?e, "Handler error during handshake");

                        // Handle QUIT specially - disconnect pre-registration client
                        if let crate::handlers::HandlerError::Quit(quit_msg) = e {
                            let error_text = match quit_msg {
                                Some(msg) => {
                                    format!("Closing Link: {} (Quit: {})", self.addr.ip(), msg)
                                }
                                None => format!("Closing Link: {} (Client Quit)", self.addr.ip()),
                            };
                            let error_reply = Message {
                                tags: None,
                                prefix: None,
                                command: Command::ERROR(error_text),
                            };
                            let _ = self.transport.write_message(&error_reply).await;

                            // Release nick if it was reserved during handshake
                            if let Some(nick) = &unreg_state.nick {
                                let nick_lower = irc_to_lower(nick);
                                self.matrix.nicks.remove(&nick_lower);
                                info!(nick = %nick, "Pre-registration nick released");
                            }
                            return Ok(()); // Disconnect
                        }

                        // Handle AccessDenied - error already sent, drain messages and disconnect
                        if matches!(e, crate::handlers::HandlerError::AccessDenied) {
                            // Drain and write queued error messages before disconnecting
                            while let Ok(response) = handshake_rx.try_recv() {
                                let _ = self.transport.write_message(&response).await;
                            }

                            // Release nick if it was reserved during handshake
                            if let Some(nick) = &unreg_state.nick {
                                let nick_lower = irc_to_lower(nick);
                                self.matrix.nicks.remove(&nick_lower);
                            }
                            return Ok(());
                        }

                        // Send appropriate error reply based on error type
                        let nick = unreg_state.nick.as_deref().unwrap_or("*");
                        if let Some(reply) = handler_error_to_reply(
                            &self.matrix.server_info.name,
                            nick,
                            &e,
                            &msg_ref,
                        ) {
                            let _ = self.transport.write_message(&reply).await;
                        }
                        // NotRegistered during handshake shouldn't break - client may just be
                        // trying commands before completing registration, which is common
                    }

                    // Drain and write queued responses synchronously
                    while let Ok(response) = handshake_rx.try_recv() {
                        if let Err(e) = self.transport.write_message(&response).await {
                            warn!(error = ?e, "Write error during handshake");
                            // Release nick if reserved during handshake
                            if let Some(nick) = &unreg_state.nick {
                                let nick_lower = irc_to_lower(nick);
                                self.matrix.nicks.remove(&nick_lower);
                            }
                            return Ok(());
                        }
                    }

                    // Check if registration is now possible (e.g., CAP END was received)
                    // This handles the case where CAP negotiation delayed registration.
                    if unreg_state.can_register() && !self.matrix.users.contains_key(&self.uid) {
                        // Re-create context to call welcome burst
                        let mut ctx = Context {
                            uid: &self.uid,
                            matrix: &self.matrix,
                            sender: ResponseMiddleware::Direct(&handshake_tx),
                            state: &mut unreg_state,
                            db: &self.db,
                            remote_addr: self.addr,
                            label: None,
                            suppress_labeled_ack: false,
                            registry: &self.registry,
                        };
                        if let Err(e) = send_welcome_burst(&mut ctx).await {
                            warn!(error = ?e, "Failed to send welcome burst");
                            // Release nick if reserved
                            if let Some(nick) = &unreg_state.nick {
                                let nick_lower = irc_to_lower(nick);
                                self.matrix.nicks.remove(&nick_lower);
                            }
                            return Ok(());
                        }
                        // Drain welcome burst messages
                        while let Ok(response) = handshake_rx.try_recv() {
                            if let Err(e) = self.transport.write_message(&response).await {
                                warn!(error = ?e, "Write error sending welcome burst");
                                if let Some(nick) = &unreg_state.nick {
                                    let nick_lower = irc_to_lower(nick);
                                    self.matrix.nicks.remove(&nick_lower);
                                }
                                return Ok(());
                            }
                        }
                    }

                    // Check if handshake is complete - can we transition to RegisteredState?
                    if self.matrix.users.contains_key(&self.uid) {
                        break;
                    }
                }
                Some(Err(e)) => {
                    match classify_read_error(&e) {
                        ReadErrorAction::InputTooLong => {
                            // Recoverable: send ERR_INPUTTOOLONG (417) and continue
                            warn!("Input line too long during handshake");
                            let nick = unreg_state.nick.as_deref().unwrap_or("*");
                            let reply = Message {
                                tags: None,
                                prefix: Some(Prefix::ServerName(
                                    self.matrix.server_info.name.clone(),
                                )),
                                command: Command::Response(
                                    Response::ERR_INPUTTOOLONG,
                                    vec![nick.to_string(), "Input line too long".to_string()],
                                ),
                            };
                            let _ = self.transport.write_message(&reply).await;
                            // Continue reading - don't disconnect
                            continue;
                        }
                        ReadErrorAction::FatalProtocolError { error_msg } => {
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
                    // Release nick if reserved during handshake
                    if let Some(nick) = &unreg_state.nick {
                        let nick_lower = irc_to_lower(nick);
                        self.matrix.nicks.remove(&nick_lower);
                    }
                    return Ok(());
                }
                None => {
                    info!("Client disconnected during handshake");
                    // Release nick if reserved during handshake
                    if let Some(nick) = &unreg_state.nick {
                        let nick_lower = irc_to_lower(nick);
                        self.matrix.nicks.remove(&nick_lower);
                    }
                    return Ok(());
                }
            }
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

        // Phase 2: Unified Zero-Copy Loop
        // Transport handles both reading and writing with unified API

        // Penalty box: Track consecutive rate limit violations
        let mut flood_violations = 0u8;
        const MAX_FLOOD_VIOLATIONS: u8 = 3; // Strike limit before disconnect

        // Track quit message for broadcast during cleanup
        let mut quit_message: Option<String> = None;

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
                            // Flood protection using global rate limiter
                            if !self.matrix.rate_limiter.check_message_rate(&self.uid) {
                                flood_violations += 1;
                                crate::metrics::RATE_LIMITED.inc();
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
                                    )).with_prefix(Prefix::ServerName(self.matrix.server_info.name.clone()));
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

                            // Check if message should be absorbed into an active batch
                            // (draft/multiline: PRIVMSG/NOTICE with batch=ref tag)
                            match process_batch_message(&mut reg_state, &msg_ref, &self.matrix.server_info.name) {
                                Ok(Some(_batch_ref)) => {
                                    // Message was consumed by the batch, don't dispatch
                                    debug!("Message absorbed into active batch");
                                    continue;
                                }
                                Ok(None) => {
                                    // Not a batch message, proceed with normal dispatch
                                }
                                Err(fail_msg) => {
                                    // Batch error - send FAIL and abort the batch
                                    warn!(error = %fail_msg, "Batch processing error");
                                    reg_state.active_batch = None;
                                    reg_state.active_batch_ref = None;
                                    // Parse and send the FAIL message
                                    if let Ok(fail) = fail_msg.parse::<Message>() {
                                        let _ = outgoing_tx.send(fail).await;
                                    }
                                    continue;
                                }
                            }

                            // Extract label tag for labeled-response (IRCv3)
                            let label = if reg_state.capabilities.contains("labeled-response") {
                                msg_ref.tags_iter()
                                    .find(|(k, _)| *k == "label")
                                    .map(|(_, v)| v.to_string())
                            } else {
                                None
                            };

                            // Select middleware: direct or capturing buffer when label is present
                            let capture_buffer: Option<Mutex<Vec<Message>>> = label.as_ref().map(|_| Mutex::new(Vec::new()));
                            let sender_middleware = if let Some(buf) = capture_buffer.as_ref() {
                                ResponseMiddleware::Capturing(buf)
                            } else {
                                ResponseMiddleware::Direct(&outgoing_tx)
                            };
                            let dispatch_sender = sender_middleware.clone();

                            // Dispatch to handler using RegisteredState context
                            let (dispatch_result, suppress_ack) = {
                                let mut ctx = Context {
                                    uid: &self.uid,
                                    matrix: &self.matrix,
                                    sender: dispatch_sender,
                                    state: &mut reg_state,
                                    db: &self.db,
                                    remote_addr: self.addr,
                                    label: label.clone(),
                                    suppress_labeled_ack: false,
                                    registry: &self.registry,
                                };

                                let result = self.registry.dispatch_post_reg(&mut ctx, &msg_ref).await;
                                (result, ctx.suppress_labeled_ack)
                            };

                            if let Err(e) = dispatch_result {
                                debug!(error = ?e, "Handler error");

                                // Handle QUIT specially - send ERROR and disconnect
                                if let crate::handlers::HandlerError::Quit(quit_msg) = e {
                                    // Store quit message for cleanup broadcast
                                    quit_message = quit_msg.clone();

                                    let error_text = match quit_msg {
                                        Some(msg) => format!("Closing Link: {} (Quit: {})", self.addr.ip(), msg),
                                        None => format!("Closing Link: {} (Client Quit)", self.addr.ip()),
                                    };
                                    let error_reply = Message {
                                        tags: None,
                                        prefix: None,
                                        command: Command::ERROR(error_text),
                                    };
                                    let _ = self.transport.write_message(&error_reply).await;
                                    break;
                                }

                                // Handle AccessDenied - error already sent, just disconnect
                                if matches!(e, crate::handlers::HandlerError::AccessDenied) {
                                    break;
                                }

                                // Send appropriate error reply based on error type
                                // RegisteredState guarantees nick is present
                                let nick = &reg_state.nick;
                                if let Some(reply) = handler_error_to_reply(&self.matrix.server_info.name, nick, &e, &msg_ref) {
                                    let _ = sender_middleware.send(reply).await;
                                }
                                // NotRegistered post-registration indicates a bug - should not happen
                                if matches!(e, crate::handlers::HandlerError::NotRegistered) {
                                    warn!("NotRegistered error after registration completed - this is a bug");
                                }
                            }

                            // Finalize labeled-response batching if needed
                            if let Some(buffer) = capture_buffer {
                                // Skip automatic ACK/BATCH if handler suppressed it (e.g., multiline)
                                if !suppress_ack {
                                    let mut guard = buffer.lock().await;
                                    let mut messages = guard.split_off(0);
                                    drop(guard);

                                    match messages.len() {
                                        0 => {
                                            if let Some(label_str) = label.as_deref() {
                                                let ack = labeled_ack(&self.matrix.server_info.name, label_str);
                                                let _ = outgoing_tx.send(ack).await;
                                            } else {
                                                warn!("Missing label while sending ACK");
                                            }
                                        }
                                        1 => {
                                            if let Some(msg) = messages.pop() {
                                                let tagged = with_label(msg, label.as_deref());
                                                let _ = outgoing_tx.send(tagged).await;
                                            }
                                        }
                                        _ => {
                                            if let Some(label_str) = label.as_deref() {
                                                let batch_id = self.next_batch_id();
                                                let start = Message {
                                                    tags: None,
                                                    prefix: Some(Prefix::ServerName(self.matrix.server_info.name.clone())),
                                                    command: Command::BATCH(
                                                        format!("+{}", batch_id),
                                                        Some(BatchSubCommand::CUSTOM("labeled-response".to_string())),
                                                        None,
                                                    ),
                                                }
                                                .with_tag("label", Some(label_str));

                                                let _ = outgoing_tx.send(start).await;

                                                for mut msg in messages.drain(..) {
                                                    msg = msg.with_tag("batch", Some(&batch_id));
                                                    let tagged = with_label(msg, Some(label_str));
                                                    let _ = outgoing_tx.send(tagged).await;
                                                }

                                                let end = Message {
                                                    tags: None,
                                                    prefix: Some(Prefix::ServerName(self.matrix.server_info.name.clone())),
                                                    command: Command::BATCH(format!("-{}", batch_id), None, None),
                                                };

                                                let _ = outgoing_tx.send(end).await;
                                            } else {
                                                warn!("Missing label while batching responses");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            match classify_read_error(&e) {
                                ReadErrorAction::InputTooLong => {
                                    // Recoverable: send ERR_INPUTTOOLONG (417) and continue
                                    warn!("Input line too long from client");
                                    let nick = &reg_state.nick;
                                    let reply = Message {
                                        tags: None,
                                        prefix: Some(Prefix::ServerName(self.matrix.server_info.name.clone())),
                                        command: Command::Response(
                                            Response::ERR_INPUTTOOLONG,
                                            vec![nick.to_string(), "Input line too long".to_string()],
                                        ),
                                    };
                                    let _ = self.transport.write_message(&reply).await;
                                    // Continue reading - don't disconnect
                                }
                                ReadErrorAction::FatalProtocolError { error_msg } => {
                                    warn!(error = %error_msg, "Protocol error from client");
                                    // Send ERROR message before disconnecting
                                    let error_reply = Message {
                                        tags: None,
                                        prefix: None,
                                        command: Command::ERROR(error_msg),
                                    };
                                    let _ = self.transport.write_message(&error_reply).await;
                                    break;
                                }
                                ReadErrorAction::IoError => {
                                    debug!(error = ?e, "I/O error");
                                    break;
                                }
                            }
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
                    // Check if this is an ERROR message indicating we've been killed/disconnected
                    let is_error_disconnect = matches!(&msg.command, Command::ERROR(_));

                    if let Err(e) = self.transport.write_message(&msg).await {
                        warn!(error = ?e, "Write error");
                        break;
                    }

                    // If we received an ERROR message and we're no longer in the Matrix,
                    // it means we were killed/disconnected by an external source
                    if is_error_disconnect && !self.matrix.users.contains_key(&self.uid) {
                        info!("Received disconnect signal - user removed from Matrix");
                        break;
                    }
                }
            }
        }

        // Cleanup: record WHOWAS and remove user from all channels
        if let Some(user_ref) = self.matrix.users.get(&self.uid) {
            let user = user_ref.read().await;
            let channels: Vec<String> = user.channels.iter().cloned().collect();
            let nick = user.nick.clone();
            let user_ident = user.user.clone();
            let host = user.host.clone();

            // Record WHOWAS entry before cleanup
            self.matrix
                .record_whowas(&user.nick, &user.user, &user.host, &user.realname);

            drop(user);

            // Broadcast QUIT to all channel members
            let quit_text = quit_message.unwrap_or_else(|| "Client Quit".to_string());
            let quit_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(nick.clone(), user_ident, host)),
                command: Command::QUIT(Some(quit_text)),
            };

            // Send Quit event to all channels
            for channel_lower in channels {
                if let Some(channel) = self.matrix.channels.get(&channel_lower) {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let event = crate::state::actor::ChannelEvent::Quit {
                        uid: self.uid.clone(),
                        quit_msg: quit_msg.clone(),
                        reply_tx: Some(tx),
                    };

                    if (channel.send(event).await).is_ok() {
                        if let Ok(remaining) = rx.await
                            && remaining == 0
                        {
                            self.matrix.channels.remove(&channel_lower);
                            crate::metrics::ACTIVE_CHANNELS.dec();
                        }
                    } else {
                        // Actor died
                        self.matrix.channels.remove(&channel_lower);
                    }
                }
            }
        }
        self.matrix.users.remove(&self.uid);
        crate::metrics::CONNECTED_USERS.dec();

        // Cleanup: remove nick from index and notify MONITOR watchers
        // RegisteredState guarantees nick is present
        let nick = &reg_state.nick;
        // Notify MONITOR watchers that this nick is going offline
        notify_monitors_offline(&self.matrix, nick).await;

        let nick_lower = irc_to_lower(nick);
        self.matrix.nicks.remove(&nick_lower);
        info!(nick = %nick, "Nick released");

        // Clean up this user's MONITOR entries
        cleanup_monitors(&self.matrix, &self.uid);

        // Unregister sender from Matrix
        self.matrix.unregister_sender(&self.uid);

        // Remove from rate limiter
        self.matrix.rate_limiter.remove_client(&self.uid);

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
