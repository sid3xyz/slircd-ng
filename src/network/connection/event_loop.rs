use super::context::{ConnectionContext, LifecycleChannels};
use super::error_handling::{
    ReadErrorAction, classify_read_error, extract_label_from_raw, handler_error_to_reply_owned,
};
use super::helpers::{
    batch_end_msg, batch_start_msg, closing_link_error, excess_flood_error, flood_warning_notice,
    input_too_long_response,
};
use crate::handlers::{
    Context, ResponseMiddleware, labeled_ack, process_batch_message, with_label,
};
use crate::state::RegisteredState;
use slirc_proto::{Command, Message, Prefix, Tag, generate_batch_ref};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const MAX_FLOOD_VIOLATIONS: u8 = 3;
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

/// Handle labeled-response protocol (IRCv3 spec).
async fn send_labeled_response(
    transport: &mut slirc_proto::transport::ZeroCopyTransportEnum,
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
        let batch_start = batch_start_msg(server_name, &batch_ref).with_tag("label", Some(label));
        let _ = transport.write_message(&batch_start).await;

        for msg in messages.drain(..) {
            let batched = msg.with_tag("batch", Some(&batch_ref));
            let _ = transport.write_message(&batched).await;
        }

        let batch_end = batch_end_msg(server_name, &batch_ref);
        let _ = transport.write_message(&batch_end).await;
    }
}

/// Run Phase 2: Unified event loop (post-registration).
pub async fn run_event_loop(
    conn: ConnectionContext<'_>,
    channels: LifecycleChannels<'_>,
    reg_state: &mut RegisteredState,
) -> Option<String> {
    let ConnectionContext {
        uid,
        transport,
        matrix,
        registry,
        db,
        addr,
        starttls_acceptor: _,
    } = conn;
    let LifecycleChannels {
        tx: outgoing_tx,
        rx: outgoing_rx,
    } = channels;
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
        if !matrix.user_manager.users.contains_key(uid) {
            info!(uid = %uid, "User removed from Matrix - disconnecting");
            break;
        }

        // All select results are pure data - NO transport writes inside tokio::select!
        // This avoids borrow conflicts between transport.next() and transport.write_message()
        enum SelectResult {
            /// No action needed, continue loop
            None,
            /// Write messages and continue loop
            Continue { pending_writes: Vec<Message> },
            /// Write messages and break from loop
            Break { pending_writes: Vec<Message> },
            /// Process an incoming message (boxed to avoid large enum variant)
            ProcessMessage {
                msg: Box<Message>,
                label: Option<String>,
            },
            /// Received outgoing message to send
            OutgoingMessage {
                msg: Arc<Message>,
                is_error_disconnect: bool,
            },
            /// Send a ping to the client
            SendPing,
            /// Ping timeout - disconnect
            PingTimeout { total_idle: u64 },
        }

        let select_result = tokio::select! {
            result = transport.next() => {
                match result {
                    Some(Ok(msg_ref)) => {
                        // Reset ping state on any received message
                        reg_state.last_activity = Instant::now();
                        reg_state.ping_pending = false;
                        reg_state.ping_sent_at = None;

                        // Convert to owned immediately to release the borrow
                        let msg = msg_ref.to_owned();

                        // Extract label from tags while we still have msg_ref
                        let label = if reg_state.capabilities.contains("labeled-response") {
                            msg_ref.tags_iter()
                                .find(|(k, _)| *k == "label")
                                .map(|(_, v)| v.to_string())
                        } else {
                            None
                        };

                        // Drop msg_ref as early as possible (now msg is owned)
                        drop(msg_ref);

                        // Flood protection - check rate limit
                        let flood_result = if matrix.security_manager.rate_limiter.check_message_rate(&uid.to_string()) {
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
                            FloodCheckResult::Ok => {
                                SelectResult::ProcessMessage { msg: Box::new(msg), label }
                            }
                            FloodCheckResult::RateLimited => {
                                let name = &matrix.server_info.name;
                                let notice = flood_warning_notice(name, flood_violations, MAX_FLOOD_VIOLATIONS);
                                SelectResult::Continue { pending_writes: vec![notice] }
                            }
                            FloodCheckResult::Disconnect => {
                                warn!(uid = %uid, "Maximum flood violations reached - disconnecting");
                                SelectResult::Break { pending_writes: vec![excess_flood_error()] }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        match classify_read_error(&e) {
                            ReadErrorAction::InputTooLong => {
                                warn!("Input line too long");
                                let server_name = &matrix.server_info.name;
                                let nick = &reg_state.nick;
                                let reply = input_too_long_response(server_name, nick);
                                SelectResult::Continue { pending_writes: vec![reply] }
                            }
                            ReadErrorAction::InvalidUtf8 { command_hint, raw_line, details } => {
                                warn!(command = ?command_hint, details = %details, "Invalid UTF-8 in message");
                                let command_name = command_hint.unwrap_or_else(|| "PRIVMSG".to_string());

                                // Extract label from raw bytes if present
                                let label = extract_label_from_raw(&raw_line);
                                let tags = label.map(|l| vec![Tag::new("label", Some(l))]);

                                // Send FAIL response per IRCv3 spec
                                let fail_msg = Message {
                                    tags,
                                    prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                                    command: Command::FAIL(
                                        command_name,
                                        "INVALID_UTF8".to_string(),
                                        vec![format!("Invalid UTF-8 in message: {}", details)],
                                    ),
                                };
                                SelectResult::Continue { pending_writes: vec![fail_msg] }
                            }
                            ReadErrorAction::FatalProtocolError { error_msg } => {
                                warn!(error = %error_msg, "Protocol error");
                                let cmd = Command::ERROR(error_msg);
                                let error_reply = Message { tags: None, prefix: None, command: cmd };
                                SelectResult::Break { pending_writes: vec![error_reply] }
                            }
                            ReadErrorAction::IoError => {
                                debug!(error = ?e, "I/O error");
                                SelectResult::Break { pending_writes: vec![] }
                            }
                        }
                    }
                    None => {
                        info!("Client disconnected");
                        SelectResult::Break { pending_writes: vec![] }
                    }
                }
            }

            Some(msg) = outgoing_rx.recv() => {
                let is_error_disconnect = matches!(&msg.command, Command::ERROR(_));
                SelectResult::OutgoingMessage { msg, is_error_disconnect }
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
                            SelectResult::PingTimeout { total_idle }
                        } else {
                            SelectResult::None
                        }
                    } else {
                        SelectResult::None
                    }
                } else if idle_time >= ping_interval {
                    // Client has been idle, need to send a PING
                    debug!(
                        uid = %uid,
                        nick = %reg_state.nick,
                        idle_secs = idle_time.as_secs(),
                        "Sending PING to idle client"
                    );
                    SelectResult::SendPing
                } else {
                    SelectResult::None
                }
            }
        };

        // Now we can use transport freely - no borrows held from tokio::select!
        match select_result {
            SelectResult::None => continue,

            SelectResult::Continue { pending_writes } => {
                for msg in pending_writes {
                    let _ = transport.write_message(&msg).await;
                }
                continue;
            }

            SelectResult::Break { pending_writes } => {
                for msg in pending_writes {
                    let _ = transport.write_message(&msg).await;
                }
                break;
            }

            SelectResult::OutgoingMessage {
                msg,
                is_error_disconnect,
            } => {
                if let Err(e) = transport.write_message(&msg).await {
                    warn!(error = ?e, "Write error");
                    break;
                }
                if is_error_disconnect && !matrix.user_manager.users.contains_key(uid) {
                    info!("Received disconnect signal - user removed from Matrix");
                    break;
                }
                continue;
            }

            SelectResult::SendPing => {
                let ping = Message::ping(&matrix.server_info.name);
                if let Err(e) = transport.write_message(&ping).await {
                    warn!(error = ?e, "Failed to send PING");
                    break;
                }
                reg_state.ping_pending = true;
                reg_state.ping_sent_at = Some(Instant::now());
                continue;
            }

            SelectResult::PingTimeout { total_idle } => {
                let error_msg = Message::from(Command::ERROR(format!(
                    "Closing Link: {} (Ping timeout: {} seconds)",
                    addr.ip(),
                    total_idle
                )));
                let _ = transport.write_message(&error_msg).await;
                break;
            }

            SelectResult::ProcessMessage { msg, label } => {
                debug!(raw = ?msg, "Received message");

                // Batch processing - need to create a temporary MessageRef for this
                // since process_batch_message needs a reference
                let raw_str = msg.to_string();
                let batch_result =
                    if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
                        process_batch_message(reg_state, &msg_ref, &matrix.server_info.name)
                    } else {
                        Ok(None)
                    };

                match batch_result {
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
                            let _ = outgoing_tx.send(Arc::new(fail)).await;
                        }
                        continue;
                    }
                }

                // Select middleware for labeled-response
                let capture_buffer: Option<Mutex<Vec<Message>>> =
                    label.as_ref().map(|_| Mutex::new(Vec::new()));
                let sender_middleware = if let Some(buf) = capture_buffer.as_ref() {
                    ResponseMiddleware::Capturing(buf)
                } else {
                    ResponseMiddleware::Direct(outgoing_tx)
                };
                let dispatch_sender = sender_middleware.clone();

                // Dispatch - create a temp MessageRef for the dispatch call
                let (dispatch_result, suppress_ack) = {
                    let raw_str = msg.to_string();
                    if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
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
                    } else {
                        // Should not happen, but handle gracefully
                        (Ok(()), false)
                    }
                };

                if let Err(e) = dispatch_result {
                    debug!(error = ?e, "Handler error");

                    if let crate::handlers::HandlerError::Quit(quit_msg) = e {
                        // Drain pending outgoing messages before quitting
                        while let Ok(msg) = outgoing_rx.try_recv() {
                            let _ = transport.write_message(&msg).await;
                        }

                        quit_message = quit_msg.clone();
                        let error_reply = closing_link_error(&addr, quit_msg.as_deref());
                        let _ = transport.write_message(&error_reply).await;
                        break;
                    } else {
                        // Other errors - use owned message for error reply
                        let nick = &reg_state.nick;
                        if let Some(reply) =
                            handler_error_to_reply_owned(&matrix.server_info.name, nick, &e, &msg)
                        {
                            let _ = transport.write_message(&reply).await;
                        }
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
                    )
                    .await;
                }
            }
        }
    }

    quit_message
}
