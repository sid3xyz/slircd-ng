use super::context::{ConnectionContext, LifecycleChannels};
use super::error_handling::{ReadErrorAction, classify_read_error, handler_error_to_reply_owned};
use crate::handlers::{Context, ResponseMiddleware, WelcomeBurstWriter};
use crate::state::{Matrix, UnregisteredState};
use slirc_proto::{Command, Message, Prefix, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Handshake exit condition.
#[derive(Debug)]
pub enum HandshakeExit {
    Quit(Option<String>),
    AccessDenied(Option<String>),
    WriteError(Option<String>),
    ProtocolError(Option<String>),
    Disconnected(Option<String>),
}

/// Handshake success condition.
#[derive(Debug)]
pub enum HandshakeSuccess {
    /// Connection registered as a user.
    User,
    /// Connection registered as a server.
    Server,
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
            matrix.user_manager.nicks.remove(&nick_lower);
            info!(nick = %nick, "Pre-registration nick released");
        }
    }
}

/// Run Phase 1: Handshake loop (pre-registration).
///
/// Returns RegisteredState on successful registration.
pub async fn run_handshake_loop(
    conn: ConnectionContext<'_>,
    channels: LifecycleChannels<'_>,
    unreg_state: &mut UnregisteredState,
) -> Result<HandshakeSuccess, HandshakeExit> {
    let ConnectionContext {
        uid,
        transport,
        matrix,
        registry,
        db,
        addr,
        starttls_acceptor,
    } = conn;
    let LifecycleChannels {
        tx: handshake_tx,
        rx: handshake_rx,
    } = channels;
    // Registration timeout from config
    let registration_timeout = Duration::from_secs(matrix.server_info.idle_timeouts.registration);
    let handshake_start = Instant::now();

    // If we are the initiator, send the initial handshake commands
    if let Some(init_data) = &unreg_state.initiator_data {
        info!(uid = %uid, target = ?init_data.remote_sid, "Initiating server handshake");

        // Send PASS
        // Format: PASS <password> TS 6 :<sid>
        // Note: We use Raw command because slirc-proto might not support TS6 PASS fully typed yet
        let pass_cmd = Command::Raw(
            "PASS".to_string(),
            vec![
                init_data.remote_password.clone(),
                "TS".to_string(),
                "6".to_string(),
                matrix.server_info.sid.clone(),
            ],
        );
        let pass_msg = Message::from(pass_cmd);
        transport
            .write_message(&pass_msg)
            .await
            .map_err(|_| HandshakeExit::WriteError(None))?;

        // Send CAP LS
        let cap_ls = Message::from(Command::CAP(
            None,
            slirc_proto::CapSubCommand::LS,
            Some("302".to_string()),
            None,
        ));
        transport
            .write_message(&cap_ls)
            .await
            .map_err(|_| HandshakeExit::WriteError(None))?;

        // Send SERVER
        // Format: SERVER <name> <hopcount> <sid> <info>
        let server_cmd = Command::Raw(
            "SERVER".to_string(),
            vec![
                matrix.server_info.name.clone(),
                "1".to_string(),
                matrix.server_info.sid.clone(),
                matrix.server_info.description.clone(),
            ],
        );
        let server_msg = Message::from(server_cmd);
        transport
            .write_message(&server_msg)
            .await
            .map_err(|_| HandshakeExit::WriteError(None))?;

        // Mark as server handshake
        unreg_state.is_server_handshake = true;
    }

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

        // Result type for the select - pure data, no I/O inside select
        enum HandshakeSelectResult {
            /// Received a valid message to process (boxed to avoid large enum variant)
            Message {
                msg: Box<Message>,
                label: Option<String>,
            },
            /// Read error occurred
            ReadError(ReadErrorAction),
            /// Client disconnected
            Disconnected,
            /// Timeout - just loop to check timeout at top
            Timeout,
        }

        // Wait for next message with timeout - NO transport writes inside this block!
        let select_result = {
            let result = tokio::time::timeout(remaining, transport.next()).await;

            match result {
                Ok(Some(Ok(msg_ref))) => {
                    debug!(raw = %msg_ref.raw.trim(), "Received message");

                    // Convert to owned immediately to release the borrow
                    let msg = msg_ref.to_owned();

                    // Extract label while we have msg_ref
                    let label = if unreg_state.capabilities.contains("labeled-response") {
                        msg_ref
                            .tags_iter()
                            .find(|(k, _)| *k == "label")
                            .map(|(_, v)| v.to_string())
                    } else {
                        None
                    };

                    // Drop msg_ref explicitly
                    drop(msg_ref);

                    HandshakeSelectResult::Message { msg: Box::new(msg), label }
                }
                Ok(Some(Err(e))) => {
                    HandshakeSelectResult::ReadError(classify_read_error(&e))
                }
                Ok(None) => {
                    info!("Client disconnected during handshake");
                    HandshakeSelectResult::Disconnected
                }
                Err(_) => {
                    // Timeout elapsed - loop will check at top and disconnect
                    HandshakeSelectResult::Timeout
                }
            }
        };

        // Now process the result - we can use transport freely here
        match select_result {
            HandshakeSelectResult::Timeout => continue,
            HandshakeSelectResult::Disconnected => {
                return Err(HandshakeExit::Disconnected(unreg_state.nick.clone()));
            }
            HandshakeSelectResult::ReadError(action) => {
                match action {
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
                        debug!("I/O error during handshake");
                    }
                }
                return Err(HandshakeExit::ProtocolError(unreg_state.nick.clone()));
            }
            HandshakeSelectResult::Message { msg, label } => {
                // Dispatch the command - we need a MessageRef for the dispatch
                // Create a temporary one from the owned message
                let raw_str = msg.to_string();
                let dispatch_result = if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
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

                    registry.dispatch_pre_reg(&mut ctx, &msg_ref).await
                } else {
                    // Should not happen, but handle gracefully
                    Ok(())
                };

                if let Err(e) = dispatch_result {
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

                    // Handle STARTTLS - upgrade connection to TLS
                    if matches!(e, crate::handlers::HandlerError::StartTls) {
                        // Drain queued responses first (includes RPL_STARTTLS)
                        while let Ok(response) = handshake_rx.try_recv() {
                            if let Err(write_err) = transport.write_message(&response).await {
                                warn!(error = ?write_err, "Write error before STARTTLS");
                                return Err(HandshakeExit::WriteError(unreg_state.nick.clone()));
                            }
                        }

                        // Check if we have a TLS acceptor available
                        let Some(acceptor) = starttls_acceptor else {
                            // No TLS configured - send error
                            let nick = unreg_state.nick.as_deref().unwrap_or("*");
                            let reply =
                                Response::err_starttls(nick, "TLS not configured on this server")
                                    .with_prefix(Prefix::ServerName(
                                        matrix.server_info.name.clone(),
                                    ));
                            let _ = transport.write_message(&reply).await;
                            continue;
                        };

                        // Perform the TLS upgrade using the in-place upgrade method
                        info!(uid = %uid, "Performing STARTTLS upgrade");

                        match transport.upgrade_to_tls(acceptor.clone()).await {
                            Ok(()) => {
                                unreg_state.is_tls = true;
                                info!(uid = %uid, "STARTTLS upgrade successful");
                            }
                            Err(tls_err) => {
                                warn!(uid = %uid, error = ?tls_err, "STARTTLS handshake failed");
                                // Connection is dead after failed TLS handshake - disconnect
                                return Err(HandshakeExit::ProtocolError(unreg_state.nick.clone()));
                            }
                        }
                        continue;
                    }

                    // Send appropriate error reply using owned message
                    let nick = unreg_state.nick.as_deref().unwrap_or("*");
                    if let Some(reply) =
                        handler_error_to_reply_owned(&matrix.server_info.name, nick, &e, &msg)
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
                if unreg_state.can_register() && !matrix.user_manager.users.contains_key(uid) {
                    let writer =
                        WelcomeBurstWriter::new(uid, matrix, transport, unreg_state, db, addr);
                    if let Err(e) = writer.send().await {
                        warn!(error = ?e, "Failed to send welcome burst");
                        return Err(HandshakeExit::WriteError(unreg_state.nick.clone()));
                    }
                }

                // Check if server registration is possible
                if unreg_state.can_register_server() {
                    return Ok(HandshakeSuccess::Server);
                }

                // Check if handshake complete
                if matrix.user_manager.users.contains_key(uid) {
                    return Ok(HandshakeSuccess::User);
                }
            }
        }
    }
}
