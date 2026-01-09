use super::context::ConnectionContext;
use crate::handlers::batch::process_batch_message;
use crate::handlers::{Context, ResponseMiddleware};
use crate::state::ServerState;
use slirc_crdt::clock::ServerId;
use slirc_proto::{Command, Message};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Run Phase 2: Server sync loop (post-registration).
pub async fn run_server_loop(
    conn: ConnectionContext<'_>,
    server_state: ServerState,
    send_handshake: bool,
) -> Result<(), std::io::Error> {
    let ConnectionContext {
        uid: _,
        transport,
        matrix,
        registry,
        db,
        addr,
        starttls_acceptor: _,
    } = conn;

    // Increase line length limit for server-to-server connections
    // This is necessary for large CRDT payloads (e.g. User structs)
    transport.set_max_line_len(65536);

    let sid = ServerId::new(server_state.sid.clone());
    let mut outgoing_rx = matrix
        .sync_manager
        .register_peer(
            sid.clone(),
            server_state.name.clone(),
            server_state.hopcount,
            server_state.info.clone(),
        )
        .await;

    info!("Entering server sync loop for {}", server_state.name);

    // If we are the listener (didn't initiate), we must send our credentials now.
    if send_handshake {
        info!("Sending server handshake to {}", server_state.name);

        // Find the link block for this server to get the password
        let link_block = matrix
            .config
            .links
            .iter()
            .find(|l| l.name == server_state.name);
        let password = link_block
            .map(|l| l.password.clone())
            .unwrap_or_else(|| "password".to_string());

        // Send PASS
        let pass_cmd = Command::Raw(
            "PASS".to_string(),
            vec![
                password,
                "TS".to_string(),
                "6".to_string(),
                matrix.server_info.sid.clone(),
            ],
        );
        let _ = transport.write_message(&Message::from(pass_cmd)).await;

        // Send CAP LS
        let cap_ls = Message::from(Command::CAP(
            None,
            slirc_proto::CapSubCommand::LS,
            Some("302".to_string()),
            None,
        ));
        let _ = transport.write_message(&cap_ls).await;

        // Send SERVER
        let server_cmd = Command::Raw(
            "SERVER".to_string(),
            vec![
                matrix.server_info.name.clone(),
                "1".to_string(),
                matrix.server_info.sid.clone(),
                matrix.server_info.description.clone(),
            ],
        );
        let _ = transport.write_message(&Message::from(server_cmd)).await;
    }

    // Trigger initial burst
    matrix.sync_manager.send_burst(&sid, matrix).await;

    let mut state = server_state;
    let (reply_tx, mut reply_rx) = mpsc::channel(100);

    loop {
        // Result type for the select - pure data, no I/O inside select
        enum ServerSelectResult {
            /// Received a valid message to process (boxed to avoid large enum variant)
            Message(Box<Message>),
            /// Read error occurred
            ReadError,
            /// Client disconnected
            Disconnected,
            /// Received a reply message to send
            Reply(Arc<Message>),
            /// Received an outgoing message to send
            Outgoing(Arc<Message>),
        }

        let select_result = tokio::select! {
            // Messages from the peer server
            result = transport.next() => {
                match result {
                    Some(Ok(msg_ref)) => {
                        debug!(raw = ?msg_ref, "Received message from peer server");
                        // Convert to owned immediately
                        let msg = msg_ref.to_owned();
                        drop(msg_ref);
                        ServerSelectResult::Message(Box::new(msg))
                    }
                    Some(Err(e)) => {
                        warn!(error = ?e, "Read error from peer server");
                        ServerSelectResult::ReadError
                    }
                    None => {
                        info!("Peer server disconnected");
                        ServerSelectResult::Disconnected
                    }
                }
            }
            // Replies from handlers (if any)
            Some(msg) = reply_rx.recv() => {
                ServerSelectResult::Reply(msg)
            }
            // Messages to send to the peer server
            Some(msg) = outgoing_rx.recv() => {
                ServerSelectResult::Outgoing(msg)
            }
        };

        // Now process the result - we can use transport freely here
        match select_result {
            ServerSelectResult::ReadError | ServerSelectResult::Disconnected => {
                break;
            }
            ServerSelectResult::Reply(msg) | ServerSelectResult::Outgoing(msg) => {
                if let Err(e) = transport.write_message(&msg).await {
                    warn!(error = ?e, "Write error to peer server");
                    break;
                }
            }
            ServerSelectResult::Message(msg) => {
                // Process batch messages (if any) - need a MessageRef for this
                let raw_str = msg.to_string();
                if let Ok(msg_ref) = slirc_proto::message::MessageRef::parse(&raw_str) {
                    match process_batch_message(&mut state, &msg_ref, &matrix.server_info.name) {
                        Ok(Some(_batch_ref)) => {
                            // Message consumed by batch
                            continue;
                        }
                        Ok(None) => {
                            // Not a batch message, or streaming batch (NETSPLIT)
                        }
                        Err(e) => {
                            warn!(error = %e, "Batch processing error");
                        }
                    }

                    let mut ctx = Context {
                        uid: "server", // Placeholder UID for server connection
                        matrix,
                        sender: ResponseMiddleware::Direct(&reply_tx),
                        state: &mut state,
                        db,
                        remote_addr: addr,
                        label: None,
                        suppress_labeled_ack: false,
                        active_batch_id: None,
                        registry,
                    };

                    if let Err(e) = registry.dispatch_server(&mut ctx, &msg_ref).await {
                        warn!(error = ?e, "Error dispatching server command");
                    }
                }
            }
        }
    }

    matrix.sync_manager.remove_peer(&sid).await;
    Ok(())
}
