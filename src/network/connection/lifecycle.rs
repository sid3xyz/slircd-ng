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
use slirc_proto::{Command, Message, Prefix, Response, generate_batch_ref, irc_to_lower};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info, warn};

const MAX_FLOOD_VIOLATIONS: u8 = 3;

/// Interval between ping timeout checks (seconds).
/// We don't need to check every second; 15s is responsive enough.
const PING_CHECK_INTERVAL_SECS: u64 = 15;

/// Result of flood rate check.
enum FloodCheckResult {
    /// Message allowed, continue processing
    Ok,
    /// Rate limit hit, warning sent, skip this message
    RateLimited,
    /// Max violations reached, disconnect
    Disconnect,
}

// === Message builders (reduce nesting depth) ===

/// Build a flood warning notice.
fn flood_warning_notice(server_name: &str, violations: u8, max: u8) -> Message {
    Message::from(Command::NOTICE(
        "*".to_string(),
        format!(
            "*** Warning: Flooding detected ({}/{} strikes). Slow down or you will be disconnected.",
            violations, max
        ),
    ))
    .with_prefix(Prefix::ServerName(server_name.to_string()))
}

/// Build an ERROR message for excess flood.
fn excess_flood_error() -> Message {
    Message::from(Command::ERROR("Excess Flood (Strike limit reached)".into()))
}

/// Build a QUIT closing link message.
fn closing_link_error(addr: &SocketAddr, quit_msg: Option<&str>) -> Message {
    let text = match quit_msg {
        Some(msg) => format!("Closing Link: {} (Quit: {})", addr.ip(), msg),
        None => format!("Closing Link: {} (Client Quit)", addr.ip()),
    };
    Message { tags: None, prefix: None, command: Command::ERROR(text) }
}

/// Build an input too long error response.
fn input_too_long_response(server_name: &str, nick: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::Response(
            Response::ERR_INPUTTOOLONG,
            vec![nick.to_string(), "Input line too long".to_string()],
        ),
    }
}

/// Build a BATCH start message for labeled-response.
fn batch_start_msg(server_name: &str, batch_ref: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::BATCH(
            format!("+{}", batch_ref),
            Some(slirc_proto::BatchSubCommand::CUSTOM("labeled-response".to_string())),
            None,
        ),
    }
}

/// Build a BATCH end message.
fn batch_end_msg(server_name: &str, batch_ref: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::BATCH(format!("-{}", batch_ref), None, None),
    }
}

/// Handle labeled-response protocol (IRCv3 spec).
async fn send_labeled_response(
    transport: &mut ZeroCopyTransportEnum,
    server_name: &str,
    label: &str,
    messages: &mut Vec<Message>,
    suppress_ack: bool,
) {
    let count = messages.len();
    if count == 0 {
        if !suppress_ack {
            let ack = labeled_ack(server_name, label);
            let _ = transport.write_message(&ack).await;
        }
    } else if count == 1 {
        // Safe: count == 1 guarantees exactly one message
        if let Some(msg) = messages.drain(..).next() {
            let tagged = with_label(msg, Some(label));
            let _ = transport.write_message(&tagged).await;
        }
    } else {
        // Multiple responses - wrap in BATCH
        let batch_ref = generate_batch_ref();
        let batch_start = batch_start_msg(server_name, &batch_ref)
            .with_tag("label", Some(label));
        let _ = transport.write_message(&batch_start).await;

        for msg in messages.drain(..) {
            let batched = msg.with_tag("batch", Some(&batch_ref));
            let _ = transport.write_message(&batched).await;
        }

        let batch_end = batch_end_msg(server_name, &batch_ref);
        let _ = transport.write_message(&batch_end).await;
    }
}

/// Shared connection resources used across lifecycle phases.
///
/// Groups parameters that are common to both handshake and event loop,
/// reducing function signature complexity.
pub struct ConnectionContext<'a> {
    /// The user's unique identifier.
    pub uid: &'a str,
    /// Transport for reading/writing IRC messages.
    pub transport: &'a mut ZeroCopyTransportEnum,
    /// Shared server state (users, channels, config).
    pub matrix: &'a Arc<Matrix>,
    /// Command handler registry.
    pub registry: &'a Arc<Registry>,
    /// Database for persistence (accounts, bans).
    pub db: &'a Database,
    /// Client's remote address.
    pub addr: SocketAddr,
}

/// Message channels for lifecycle phases.
pub struct LifecycleChannels<'a> {
    /// Sender for queueing outgoing messages.
    pub tx: &'a mpsc::Sender<Message>,
    /// Receiver for draining outgoing messages.
    pub rx: &'a mut mpsc::Receiver<Message>,
}

/// Run Phase 1: Handshake loop (pre-registration).
///
/// Returns RegisteredState on successful registration.
pub async fn run_handshake_loop(
    conn: ConnectionContext<'_>,
    channels: LifecycleChannels<'_>,
    unreg_state: &mut UnregisteredState,
) -> Result<(), HandshakeExit> {
    let ConnectionContext { uid, transport, matrix, registry, db, addr } = conn;
    let LifecycleChannels { tx: handshake_tx, rx: handshake_rx } = channels;
    // Registration timeout from config
    let registration_timeout = Duration::from_secs(matrix.server_info.idle_timeouts.registration);
    let handshake_start = Instant::now();

    loop {
        // Check if registration has timed out
        let elapsed = Instant::now().duration_since(handshake_start);
        if elapsed >= registration_timeout {
            warn!(uid = %uid, elapsed_secs = elapsed.as_secs(), "Registration timeout");
            let error_msg = Message {
                tags: None,
                prefix: None,
                command: Command::ERROR(format!(
                    "Closing Link: {} (Registration timeout: {} seconds)",
                    addr.ip(),
                    elapsed.as_secs()
                )),
            };
            let _ = transport.write_message(&error_msg).await;
            return Err(HandshakeExit::ProtocolError(unreg_state.nick.clone()));
        }

        // Calculate remaining time until timeout
        let remaining = registration_timeout.saturating_sub(elapsed);

        // Wait for next message with timeout
        let result = tokio::time::timeout(remaining, transport.next()).await;

        match result {
            Ok(Some(Ok(msg_ref))) => {
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
            Ok(Some(Err(e))) => {
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
            Ok(None) => {
                info!("Client disconnected during handshake");
                return Err(HandshakeExit::Disconnected(unreg_state.nick.clone()));
            }
            Err(_) => {
                // Timeout elapsed - loop will check at top and disconnect
                continue;
            }
        }
    }
}

/// Run Phase 2: Unified event loop (post-registration).
pub async fn run_event_loop(
    conn: ConnectionContext<'_>,
    channels: LifecycleChannels<'_>,
    reg_state: &mut RegisteredState,
) -> Option<String> {
    let ConnectionContext { uid, transport, matrix, registry, db, addr } = conn;
    let LifecycleChannels { tx: outgoing_tx, rx: outgoing_rx } = channels;
    let mut flood_violations = 0u8;
    let mut quit_message: Option<String> = None;

    // Ping timeout configuration
    let ping_interval = Duration::from_secs(matrix.server_info.idle_timeouts.ping);
    let ping_timeout = Duration::from_secs(matrix.server_info.idle_timeouts.timeout);
    let mut ping_check_timer = tokio::time::interval(Duration::from_secs(PING_CHECK_INTERVAL_SECS));
    // First tick fires immediately, we don't want that
    ping_check_timer.tick().await;

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
                        // Reset ping state on any received message
                        reg_state.last_activity = Instant::now();
                        reg_state.ping_pending = false;
                        reg_state.ping_sent_at = None;

                        // Flood protection - check rate limit
                        let flood_result = if matrix.rate_limiter.check_message_rate(&uid.to_string()) {
                            flood_violations = 0;
                            FloodCheckResult::Ok
                        } else {
                            flood_violations += 1;
                            crate::metrics::RATE_LIMITED.inc();
                            warn!(uid = %uid, violations = flood_violations, "Rate limit exceeded");

                            if flood_violations >= MAX_FLOOD_VIOLATIONS {
                                FloodCheckResult::Disconnect
                            } else {
                                FloodCheckResult::RateLimited
                            }
                        };

                        match flood_result {
                            FloodCheckResult::Ok => {}
                            FloodCheckResult::RateLimited => {
                                let name = &matrix.server_info.name;
                                let msg = flood_warning_notice(name, flood_violations, MAX_FLOOD_VIOLATIONS);
                                let _ = transport.write_message(&msg).await;
                                continue;
                            }
                            FloodCheckResult::Disconnect => {
                                warn!(uid = %uid, "Maximum flood violations reached - disconnecting");
                                let _ = transport.write_message(&excess_flood_error()).await;
                                break;
                            }
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
                                if let Ok(fail) = fail_msg.parse::<Message>() { let _ = outgoing_tx.send(fail).await; }
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
                                let error_reply = closing_link_error(&addr, quit_msg.as_deref());
                                let _ = transport.write_message(&error_reply).await;
                                break;
                            }

                            // Other errors
                            let nick = &reg_state.nick;
                            if let Some(reply) = handler_error_to_reply(&matrix.server_info.name, nick, &e, &msg_ref) {
                                let _ = transport.write_message(&reply).await;
                            }
                        }

                        // Labeled-response handling (IRCv3 spec compliant)
                        if let Some(label_str) = label
                            && let Some(buf) = capture_buffer
                        {
                            let mut messages = buf.lock().await;
                            send_labeled_response(
                                transport,
                                &matrix.server_info.name,
                                &label_str,
                                &mut messages,
                                suppress_ack,
                            ).await;
                        }
                    }
                    Some(Err(e)) => {
                        match classify_read_error(&e) {
                            ReadErrorAction::InputTooLong => {
                                warn!("Input line too long");
                                let server_name = &matrix.server_info.name;
                                let nick = &reg_state.nick;
                                let reply = input_too_long_response(server_name, nick);
                                let _ = transport.write_message(&reply).await;
                                continue;
                            }
                            ReadErrorAction::FatalProtocolError { error_msg } => {
                                warn!(error = %error_msg, "Protocol error");
                                let cmd = Command::ERROR(error_msg);
                                let error_reply = Message { tags: None, prefix: None, command: cmd };
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

            _ = ping_check_timer.tick() => {
                let now = Instant::now();
                let idle_time = now.duration_since(reg_state.last_activity);

                if reg_state.ping_pending {
                    // We sent a PING and are waiting for PONG
                    if let Some(sent_at) = reg_state.ping_sent_at {
                        let wait_time = now.duration_since(sent_at);
                        if wait_time >= ping_timeout {
                            // Ping timeout - disconnect
                            let total_idle = idle_time.as_secs();
                            warn!(
                                uid = %uid,
                                nick = %reg_state.nick,
                                idle_secs = total_idle,
                                "Ping timeout - disconnecting"
                            );
                            quit_message = Some(format!("Ping timeout: {} seconds", total_idle));
                            let error_msg = Message::from(Command::ERROR(
                                format!("Closing Link: {} (Ping timeout: {} seconds)", addr.ip(), total_idle)
                            ));
                            let _ = transport.write_message(&error_msg).await;
                            break;
                        }
                    }
                } else if idle_time >= ping_interval {
                    // Client has been idle, send a PING
                    debug!(
                        uid = %uid,
                        nick = %reg_state.nick,
                        idle_secs = idle_time.as_secs(),
                        "Sending PING to idle client"
                    );
                    let ping = Message::ping(&matrix.server_info.name);
                    if let Err(e) = transport.write_message(&ping).await {
                        warn!(error = ?e, "Failed to send PING");
                        break;
                    }
                    reg_state.ping_pending = true;
                    reg_state.ping_sent_at = Some(now);
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
