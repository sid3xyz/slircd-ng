//! BATCH command handler for server-to-server communication.

use super::types::BatchState;
use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerResult};
use crate::state::dashmap_ext::DashMapExt;
use crate::state::{BatchRouting, ServerState};
use async_trait::async_trait;
use slirc_proto::sync::clock::ServerId;
use slirc_proto::MessageRef;
use std::sync::Arc;
use tracing::debug;

/// Handler for BATCH command from servers.
pub struct ServerBatchHandler;

#[async_trait]
impl ServerHandler for ServerBatchHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // BATCH +ref type [params...] or BATCH -ref
        let ref_tag = msg.arg(0).unwrap_or("");

        if ref_tag.is_empty() {
            return Ok(());
        }

        if let Some(stripped) = ref_tag.strip_prefix('+') {
            // Start a new batch
            let batch_type = msg.arg(1).unwrap_or("");

            debug!(
                source = ?ctx.state.name,
                batch_ref = %stripped,
                batch_type = %batch_type,
                "Received server BATCH start"
            );

            // Store active batch state
            // Note: For server-to-server, we might not need to buffer messages like client batches.
            // We might just need to track that we are in a batch to properly tag outgoing messages
            // if we were relaying them.
            // However, for now, we just store it to satisfy the state interface.
            ctx.state.active_batch = Some(BatchState {
                batch_type: batch_type.to_string(),
                target: String::new(), // Not used for server batches yet
                command_type: None,
                lines: Vec::new(),
                total_bytes: 0,
                response_label: None,
                client_tags: Vec::new(),
            });
            ctx.state.active_batch_ref = Some(stripped.to_string());

            // Relay logic
            match batch_type {
                "NETSPLIT" => {
                    // NETSPLIT is a broadcast
                    // BATCH +ref NETSPLIT server_name
                    // We should broadcast this to other peers
                    let msg_owned = msg.to_owned();
                    let sid = ServerId::new(ctx.state.sid.clone());
                    ctx.matrix
                        .sync_manager
                        .broadcast(Arc::new(msg_owned), Some(&sid))
                        .await;
                    ctx.state.batch_routing = Some(BatchRouting::Broadcast);
                }
                "chathistory" => {
                    // CHATHISTORY is usually targeted
                    // BATCH +ref chathistory target timestamp
                    let target = msg.arg(2).unwrap_or("");
                    if !target.is_empty() {
                        let is_channel = target.starts_with('#') || target.starts_with('&');

                        if is_channel {
                            // Broadcast to all servers (simplest for now)
                            let msg_owned = msg.to_owned();
                            let sid = ServerId::new(ctx.state.sid.clone());
                            ctx.matrix
                                .sync_manager
                                .broadcast(Arc::new(msg_owned), Some(&sid))
                                .await;
                            ctx.state.batch_routing = Some(BatchRouting::Broadcast);
                        } else {
                            // User target
                            // Try to resolve nick to UID
                            let uid = if let Some(u) = ctx.matrix.user_manager.nicks.get(target) {
                                Some(u.value().clone())
                            } else if ctx.matrix.user_manager.users.contains_key(target) {
                                Some(target.to_string())
                            } else {
                                None
                            };

                            if let Some(uid) = uid {
                                // Extract SID from UID (first 3 chars)
                                if uid.len() >= 3 {
                                    let sid_str = &uid[0..3];
                                    let target_sid = ServerId::new(sid_str.to_string());

                                    if target_sid.as_str()
                                        == ctx.matrix.sync_manager.local_id.as_str()
                                    {
                                        // Local user
                                        debug!(
                                            "Received CHATHISTORY batch for local user {}",
                                            target
                                        );
                                        // Send batch start to local user
                                        if let Some(sender) =
                                            ctx.matrix.user_manager.senders.get_cloned(&uid)
                                        {
                                            let msg_owned = msg.to_owned();
                                            let _ = sender.send(Arc::new(msg_owned)).await;
                                            ctx.state.batch_routing =
                                                Some(BatchRouting::Local(uid));
                                        } else {
                                            debug!("Local user {} not found in senders", uid);
                                            ctx.state.batch_routing = Some(BatchRouting::None);
                                        }
                                    } else {
                                        // Remote user - Route it
                                        if let Some(peer) =
                                            ctx.matrix.sync_manager.get_next_hop(&target_sid)
                                        {
                                            let msg_owned = msg.to_owned();
                                            let _ = peer.tx.send(Arc::new(msg_owned)).await;
                                            ctx.state.batch_routing =
                                                Some(BatchRouting::Routed(target_sid));
                                        } else {
                                            debug!(
                                                "No route to server {} for user {}",
                                                target_sid.as_str(),
                                                target
                                            );
                                            ctx.state.batch_routing = Some(BatchRouting::None);
                                        }
                                    }
                                } else {
                                    debug!("Invalid UID format for {}", target);
                                    ctx.state.batch_routing = Some(BatchRouting::None);
                                }
                            } else {
                                debug!("Unknown target for CHATHISTORY batch: {}", target);
                                ctx.state.batch_routing = Some(BatchRouting::None);
                            }
                        }
                    } else {
                        ctx.state.batch_routing = Some(BatchRouting::None);
                    }
                }
                _ => {
                    // Unknown batch type, maybe just broadcast?
                    // Safer to ignore or log for now to avoid storms
                    debug!("Unknown server batch type: {}", batch_type);
                    ctx.state.batch_routing = Some(BatchRouting::None);
                }
            }
        } else if let Some(stripped) = ref_tag.strip_prefix('-') {
            // End a batch
            debug!(
                source = ?ctx.state.name,
                batch_ref = %stripped,
                "Received server BATCH end"
            );

            // Relay logic based on stored decision
            if let Some(routing) = &ctx.state.batch_routing {
                match routing {
                    BatchRouting::Broadcast => {
                        let msg_owned = msg.to_owned();
                        let sid = ServerId::new(ctx.state.sid.clone());
                        ctx.matrix
                            .sync_manager
                            .broadcast(Arc::new(msg_owned), Some(&sid))
                            .await;
                    }
                    BatchRouting::Routed(target_sid) => {
                        // Route to specific server
                        if let Some(peer) = ctx.matrix.sync_manager.get_next_hop(target_sid) {
                            let msg_owned = msg.to_owned();
                            let _ = peer.tx.send(Arc::new(msg_owned)).await;
                        }
                    }
                    BatchRouting::Local(uid) => {
                        // Route to local user
                        if let Some(sender) = ctx.matrix.user_manager.senders.get_cloned(uid) {
                            let msg_owned = msg.to_owned();
                            let _ = sender.send(Arc::new(msg_owned)).await;
                        }
                    }
                    BatchRouting::None => {}
                }
            }

            // Clear active batch state
            if ctx.state.active_batch_ref.as_deref() == Some(stripped) {
                ctx.state.active_batch = None;
                ctx.state.active_batch_ref = None;
                ctx.state.batch_routing = None;
            }
        }

        Ok(())
    }
}
