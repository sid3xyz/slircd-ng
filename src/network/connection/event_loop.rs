use super::context::{ConnectionContext, LifecycleChannels};
use super::error_handling::{ReadErrorAction, classify_read_error, handler_error_to_reply};
use super::helpers::{
    batch_end_msg, batch_start_msg, closing_link_error, excess_flood_error, flood_warning_notice,
    input_too_long_response,
};
use crate::handlers::{
    Context, ResponseMiddleware, labeled_ack, process_batch_message, with_label,
};
use crate::state::RegisteredState;
use slirc_proto::{Command, Message, generate_batch_ref};
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

        tokio::select! {
            result = transport.next() => {
                match result {
                    Some(Ok(msg_ref)) => {
                        // Reset ping state on any received message
                        reg_state.last_activity = Instant::now();
                        reg_state.ping_pending = false;
                        reg_state.ping_sent_at = None;

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

                if is_error_disconnect && !matrix.user_manager.users.contains_key(uid) {
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
