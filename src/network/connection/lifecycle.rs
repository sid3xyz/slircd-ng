//! Connection lifecycle orchestration.
//!
//! Manages the two-phase connection lifecycle:
//! - Phase 1: Handshake (capability negotiation, registration)
//! - Phase 2: Unified event loop (command processing, message routing)

use super::error_handling::{ReadErrorAction, classify_read_error, handler_error_to_reply};
use crate::db::Database;
use crate::handlers::{Context, Registry, ResponseMiddleware, WelcomeBurstWriter, process_batch_message, labeled_ack, with_label};
use crate::state::{Matrix, RegisteredState, UnregisteredState};
use slirc_proto::transport::ZeroCopyTransportEnum;
use slirc_proto::{Command, Message, Prefix, Response, irc_to_lower};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info, warn};

const MAX_FLOOD_VIOLATIONS: u8 = 3;

/// Run Phase 1: Handshake loop (pre-registration).
///
/// Returns RegisteredState on successful registration.
#[allow(clippy::too_many_arguments)]
pub async fn run_handshake_loop(
    uid: &str,
    transport: &mut ZeroCopyTransportEnum,
    matrix: &Arc<Matrix>,
    registry: &Arc<Registry>,
    db: &Database,
    addr: SocketAddr,
    unreg_state: &mut UnregisteredState,
    handshake_tx: &mpsc::Sender<Message>,
    handshake_rx: &mut mpsc::Receiver<Message>,
) -> Result<(), HandshakeExit> {
    loop {
        match transport.next().await {
            Some(Ok(msg_ref)) => {
                debug!(raw = %msg_ref.raw.trim(), "Received message");

                let label = if unreg_state.capabilities.contains("labeled-response") {
                    msg_ref
                        .tags_iter()
                        .find(|(k, _)| *k == "label")
                        .map(|(_, v)| v.to_string())
                } else {
                    None
                };

                let mut ctx = Context {
                    uid,
                    matrix,
                    sender: ResponseMiddleware::Direct(handshake_tx),
                    state: unreg_state,
                    db,
                    remote_addr: addr,
                    label,
                    suppress_labeled_ack: false,
                    active_batch_id: None,
                    registry,
                };

                if let Err(e) = registry.dispatch_pre_reg(&mut ctx, &msg_ref).await {
                    debug!(error = ?e, "Handler error during handshake");

                    // Handle QUIT - disconnect pre-registration
                    if let crate::handlers::HandlerError::Quit(quit_msg) = e {
                        let error_text = match quit_msg {
                            Some(msg) => {
                                format!("Closing Link: {} (Quit: {})", addr.ip(), msg)
                            }
                            None => format!("Closing Link: {} (Client Quit)", addr.ip()),
                        };
                        let error_reply = Message {
                            tags: None,
                            prefix: None,
                            command: Command::ERROR(error_text),
                        };
                        let _ = transport.write_message(&error_reply).await;

                        return Err(HandshakeExit::Quit(unreg_state.nick.clone()));
                    }

                    // Handle AccessDenied - drain and disconnect
                    if matches!(e, crate::handlers::HandlerError::AccessDenied) {
                        while let Ok(response) = handshake_rx.try_recv() {
                            let _ = transport.write_message(&response).await;
                        }
                        return Err(HandshakeExit::AccessDenied(unreg_state.nick.clone()));
                    }

                    // Send appropriate error reply
                    let nick = unreg_state.nick.as_deref().unwrap_or("*");
                    if let Some(reply) =
                        handler_error_to_reply(&matrix.server_info.name, nick, &e, &msg_ref)
                    {
                        let _ = transport.write_message(&reply).await;
                    }
                }

                // Drain queued responses
                while let Ok(response) = handshake_rx.try_recv() {
                    if let Err(e) = transport.write_message(&response).await {
                        warn!(error = ?e, "Write error during handshake");
                        return Err(HandshakeExit::WriteError(unreg_state.nick.clone()));
                    }
                }

                // Check if registration is possible
                if unreg_state.can_register() && !matrix.users.contains_key(uid) {
                    let writer =
                        WelcomeBurstWriter::new(uid, matrix, transport, unreg_state, db, addr);
                    if let Err(e) = writer.send().await {
                        warn!(error = ?e, "Failed to send welcome burst");
                        return Err(HandshakeExit::WriteError(unreg_state.nick.clone()));
                    }
                }

                // Check if handshake complete
                if matrix.users.contains_key(uid) {
                    return Ok(());
                }
            }
            Some(Err(e)) => {
                match classify_read_error(&e) {
                    ReadErrorAction::InputTooLong => {
                        warn!("Input line too long during handshake");
                        let nick = unreg_state.nick.as_deref().unwrap_or("*");
                        let reply = Message {
                            tags: None,
                            prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                            command: Command::Response(
                                Response::ERR_INPUTTOOLONG,
                                vec![nick.to_string(), "Input line too long".to_string()],
                            ),
                        };
                        let _ = transport.write_message(&reply).await;
                        continue;
                    }
                    ReadErrorAction::FatalProtocolError { error_msg } => {
                        warn!(error = %error_msg, "Protocol error during handshake");
                        let error_reply = Message {
                            tags: None,
                            prefix: None,
                            command: Command::ERROR(error_msg),
                        };
                        let _ = transport.write_message(&error_reply).await;
                    }
                    ReadErrorAction::IoError => {
                        debug!(error = ?e, "I/O error during handshake");
                    }
                }
                return Err(HandshakeExit::ProtocolError(unreg_state.nick.clone()));
            }
            None => {
                info!("Client disconnected during handshake");
                return Err(HandshakeExit::Disconnected(unreg_state.nick.clone()));
            }
        }
    }
}

/// Run Phase 2: Unified event loop (post-registration).
#[allow(clippy::too_many_arguments)]
pub async fn run_event_loop(
    uid: &str,
    transport: &mut ZeroCopyTransportEnum,
    matrix: &Arc<Matrix>,
    registry: &Arc<Registry>,
    db: &Database,
    addr: SocketAddr,
    reg_state: &mut RegisteredState,
    outgoing_tx: &mpsc::Sender<Message>,
    outgoing_rx: &mut mpsc::Receiver<Message>,
) -> Option<String> {
    let mut flood_violations = 0u8;
    let mut quit_message: Option<String> = None;

    info!("Entering unified event loop");

    loop {
        if !matrix.users.contains_key(uid) {
            info!(uid = %uid, "User removed from Matrix - disconnecting");
            break;
        }

        tokio::select! {
            result = transport.next() => {
                match result {
                    Some(Ok(msg_ref)) => {
                        // Flood protection
                        if !matrix.rate_limiter.check_message_rate(&uid.to_string()) {
                            flood_violations += 1;
                            crate::metrics::RATE_LIMITED.inc();
                            warn!(uid = %uid, violations = flood_violations, "Rate limit exceeded");

                            if flood_violations >= MAX_FLOOD_VIOLATIONS {
                                warn!(uid = %uid, "Maximum flood violations reached - disconnecting");
                                let error_msg = Message::from(Command::ERROR("Excess Flood (Strike limit reached)".into()));
                                let _ = transport.write_message(&error_msg).await;
                                break;
                            } else {
                                let notice = Message::from(Command::NOTICE(
                                    "*".to_string(),
                                    format!("*** Warning: Flooding detected ({}/{} strikes). Slow down or you will be disconnected.",
                                            flood_violations, MAX_FLOOD_VIOLATIONS)
                                )).with_prefix(Prefix::ServerName(matrix.server_info.name.clone()));
                                let _ = transport.write_message(&notice).await;

                                let penalty_ms = 500 * (flood_violations as u64);
                                tokio::time::sleep(tokio::time::Duration::from_millis(penalty_ms)).await;
                                continue;
                            }
                        } else {
                            flood_violations = 0;
                        }

                        debug!(raw = ?msg_ref, "Received message (zero-copy)");

                        // Batch processing
                        match process_batch_message(reg_state, &msg_ref, &matrix.server_info.name) {
                            Ok(Some(_batch_ref)) => {
                                debug!("Message absorbed into active batch");
                                continue;
                            }
                            Ok(None) => {}
                            Err(fail_msg) => {
                                warn!(error = %fail_msg, "Batch processing error");
                                reg_state.active_batch = None;
                                reg_state.active_batch_ref = None;
                                if let Ok(fail) = fail_msg.parse::<Message>() {
                                    let _ = outgoing_tx.send(fail).await;
                                }
                                continue;
                            }
                        }

                        // Extract label
                        let label = if reg_state.capabilities.contains("labeled-response") {
                            msg_ref.tags_iter()
                                .find(|(k, _)| *k == "label")
                                .map(|(_, v)| v.to_string())
                        } else {
                            None
                        };

                        // Select middleware
                        let capture_buffer: Option<Mutex<Vec<Message>>> = label.as_ref().map(|_| Mutex::new(Vec::new()));
                        let sender_middleware = if let Some(buf) = capture_buffer.as_ref() {
                            ResponseMiddleware::Capturing(buf)
                        } else {
                            ResponseMiddleware::Direct(outgoing_tx)
                        };
                        let dispatch_sender = sender_middleware.clone();

                        // Dispatch
                        let (dispatch_result, suppress_ack) = {
                            let mut ctx = Context {
                                uid,
                                matrix,
                                sender: dispatch_sender,
                                state: reg_state,
                                db,
                                remote_addr: addr,
                                label: label.clone(),
                                suppress_labeled_ack: false,
                                active_batch_id: None,
                                registry,
                            };

                            let result = registry.dispatch_post_reg(&mut ctx, &msg_ref).await;
                            (result, ctx.suppress_labeled_ack)
                        };

                        if let Err(e) = dispatch_result {
                            debug!(error = ?e, "Handler error");

                            if let crate::handlers::HandlerError::Quit(quit_msg) = e {
                                quit_message = quit_msg.clone();
                                let error_text = match quit_msg {
                                    Some(msg) => format!("Closing Link: {} (Quit: {})", addr.ip(), msg),
                                    None => format!("Closing Link: {} (Client Quit)", addr.ip()),
                                };
                                let error_reply = Message {
                                    tags: None,
                                    prefix: None,
                                    command: Command::ERROR(error_text),
                                };
                                let _ = transport.write_message(&error_reply).await;
                                break;
                            }

                            // Other errors
                            let nick = &reg_state.nick;
                            if let Some(reply) = handler_error_to_reply(&matrix.server_info.name, nick, &e, &msg_ref) {
                                let _ = transport.write_message(&reply).await;
                            }
                        }

                        // Labeled-response handling
                        if let Some(label_str) = label
                            && let Some(buf) = capture_buffer
                        {
                            let mut messages = buf.lock().await;
                            if !messages.is_empty() {
                                for msg in messages.drain(..) {
                                    let tagged = with_label(msg, Some(&label_str));
                                    let _ = transport.write_message(&tagged).await;
                                }
                            } else if !suppress_ack {
                                let ack = labeled_ack(&matrix.server_info.name, &label_str);
                                let _ = transport.write_message(&ack).await;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        match classify_read_error(&e) {
                            ReadErrorAction::InputTooLong => {
                                warn!("Input line too long");
                                let reply = Message {
                                    tags: None,
                                    prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                                    command: Command::Response(
                                        Response::ERR_INPUTTOOLONG,
                                        vec![reg_state.nick.clone(), "Input line too long".to_string()],
                                    ),
                                };
                                let _ = transport.write_message(&reply).await;
                                continue;
                            }
                            ReadErrorAction::FatalProtocolError { error_msg } => {
                                warn!(error = %error_msg, "Protocol error");
                                let error_reply = Message {
                                    tags: None,
                                    prefix: None,
                                    command: Command::ERROR(error_msg),
                                };
                                let _ = transport.write_message(&error_reply).await;
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

            Some(msg) = outgoing_rx.recv() => {
                let is_error_disconnect = matches!(&msg.command, Command::ERROR(_));

                if let Err(e) = transport.write_message(&msg).await {
                    warn!(error = ?e, "Write error");
                    break;
                }

                if is_error_disconnect && !matrix.users.contains_key(uid) {
                    info!("Received disconnect signal - user removed from Matrix");
                    break;
                }
            }
        }
    }

    quit_message
}

/// Handshake exit condition.
#[derive(Debug)]
pub enum HandshakeExit {
    Quit(Option<String>),
    AccessDenied(Option<String>),
    WriteError(Option<String>),
    ProtocolError(Option<String>),
    Disconnected(Option<String>),
}

impl HandshakeExit {
    pub fn nick(&self) -> Option<&str> {
        match self {
            HandshakeExit::Quit(n)
            | HandshakeExit::AccessDenied(n)
            | HandshakeExit::WriteError(n)
            | HandshakeExit::ProtocolError(n)
            | HandshakeExit::Disconnected(n) => n.as_deref(),
        }
    }

    pub fn release_nick(&self, matrix: &Matrix) {
        if let Some(nick) = self.nick() {
            let nick_lower = irc_to_lower(nick);
            matrix.nicks.remove(&nick_lower);
            info!(nick = %nick, "Pre-registration nick released");
        }
    }
}
