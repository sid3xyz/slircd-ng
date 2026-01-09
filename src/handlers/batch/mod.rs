//! BATCH command handler for IRCv3 message batching.
//!
//! Handles client-to-server batches, particularly draft/multiline for
//! sending multi-line messages.
//!
//! Reference: <https://ircv3.net/specs/extensions/batch>
//! Reference: <https://ircv3.net/specs/extensions/multiline>

mod processing;
pub mod server;
mod types;
mod validation;

// Re-export public types
pub use types::BatchState;

// Re-export processing function
pub use processing::process_batch_message;

use super::{
    Context, HandlerResult, PostRegHandler, ResponseMiddleware, resolve_nick_or_nosuchnick,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{
    BatchSubCommand, ChannelExt, Command, Message, MessageRef, Prefix, Response, Tag,
    format_server_time, generate_batch_ref, generate_msgid,
};
use tracing::debug;

/// Handler for BATCH command.
pub struct BatchHandler;

#[async_trait]
impl PostRegHandler for BatchHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // BATCH +ref type [params...] or BATCH -ref
        let ref_tag = msg.arg(0).unwrap_or("");

        if ref_tag.is_empty() {
            return Ok(());
        }

        let nick = ctx.nick().to_string();

        if let Some(stripped) = ref_tag.strip_prefix('+') {
            // Start a new batch
            let batch_type = msg.arg(1).unwrap_or("");
            let target = msg.arg(2).unwrap_or("");

            if batch_type.eq_ignore_ascii_case("draft/multiline") {
                // Check if client has the capability
                if !ctx.state.capabilities.contains("draft/multiline") {
                    send_fail(ctx, "MULTILINE_INVALID", "Capability not negotiated").await?;
                    return Ok(());
                }

                if target.is_empty() {
                    send_fail(ctx, "MULTILINE_INVALID", "No target specified").await?;
                    return Ok(());
                }

                // Store batch state in handshake
                debug!(nick = %nick, batch_ref = %stripped, target = %target, "Starting multiline batch");

                // Save the response label for when we complete the batch
                let response_label = ctx.label.clone();

                // Extract client-only tags (tags starting with '+') from BATCH + message
                let client_tags: Vec<Tag> = msg
                    .tags_iter()
                    .filter(|(key, _)| key.starts_with('+'))
                    .map(|(key, value)| {
                        let val = if value.is_empty() {
                            None
                        } else {
                            Some(value.to_string())
                        };
                        Tag::new(key, val)
                    })
                    .collect();

                // Store in context
                ctx.state.active_batch = Some(BatchState {
                    batch_type: "draft/multiline".to_string(),
                    target: target.to_string(),
                    lines: Vec::new(),
                    total_bytes: 0,
                    command_type: None,
                    response_label,
                    client_tags,
                });
                ctx.state.active_batch_ref = Some(stripped.to_string());

                // CRITICAL: Suppress the automatic labeled-response ACK for BATCH +
                // The label will be applied manually to the BATCH echo when BATCH - is processed
                ctx.suppress_labeled_ack = true;
            }
        } else if let Some(stripped) = ref_tag.strip_prefix('-') {
            // End a batch
            if let Some(ref active_ref) = ctx.state.active_batch_ref {
                if active_ref != stripped {
                    send_fail(ctx, "MULTILINE_INVALID", "Batch reference mismatch").await?;
                    return Ok(());
                }
            } else {
                // No active batch
                return Ok(());
            }

            // Process the completed batch
            if let Some(batch) = ctx.state.active_batch.take() {
                ctx.state.active_batch_ref = None;

                if batch.batch_type == "draft/multiline" {
                    process_multiline_batch(ctx, &batch, &nick).await?;
                }
            }
        }

        Ok(())
    }
}

/// Process a completed multiline batch by delivering to recipients.
async fn process_multiline_batch(
    ctx: &mut Context<'_, RegisteredState>,
    batch: &BatchState,
    nick: &str,
) -> HandlerResult {
    // Validate batch isn't empty or all blank
    if let Err(msg) = validation::validate_batch_not_empty(batch) {
        send_fail(ctx, "MULTILINE_INVALID", msg).await?;
        return Ok(());
    }

    // Build the combined message
    // total_bytes is the sum of line content lengths; add extra for newlines
    let mut combined = String::with_capacity(batch.total_bytes + batch.lines.len());
    for (i, line) in batch.lines.iter().enumerate() {
        if i > 0 && !line.concat {
            combined.push('\n');
        }
        combined.push_str(&line.content);
    }

    debug!(
        nick = %nick,
        target = %batch.target,
        lines = batch.lines.len(),
        bytes = combined.len(),
        "Processing completed multiline batch"
    );

    // Get the command type
    let cmd_type = batch.command_type.as_deref().unwrap_or("PRIVMSG");

    // Now we need to deliver this to recipients
    // For clients with draft/multiline, send as a batch
    // For clients without, send as individual lines (fallback)

    let target = &batch.target;

    // Get sender info for prefix
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.value().clone());
    let prefix = if let Some(user_arc) = user_arc {
        let user = user_arc.read().await;
        Prefix::new(
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
        )
    } else {
        Prefix::new(nick.to_string(), "user".to_string(), "host".to_string())
    };

    // Determine if target is a channel or user
    let is_channel = target.is_channel_name();

    if is_channel {
        // Channel message - deliver to all members
        deliver_multiline_to_channel(ctx, batch, &combined, &prefix, cmd_type).await?;
    } else {
        // Private message - deliver to single user
        deliver_multiline_to_user(ctx, batch, &combined, &prefix, cmd_type, target).await?;
    }

    Ok(())
}

/// Deliver a multiline batch to a channel.
async fn deliver_multiline_to_channel(
    ctx: &mut Context<'_, RegisteredState>,
    batch: &BatchState,
    _combined: &str,
    prefix: &Prefix,
    cmd_type: &str,
) -> HandlerResult {
    let channel_lower = batch.target.to_lowercase();

    let channel_tx = ctx
        .matrix
        .channel_manager
        .channels
        .get(&channel_lower)
        .map(|c| c.clone());
    let Some(channel_tx) = channel_tx else {
        // Channel doesn't exist - send error
        let reply = Response::err_nosuchchannel(&ctx.state.nick, &batch.target)
            .with_prefix(ctx.server_prefix());
        ctx.send_error("BATCH", "ERR_NOSUCHCHANNEL", reply).await?;
        return Ok(());
    };

    // Get list of members
    let (tx, rx) = tokio::sync::oneshot::channel();
    if (channel_tx
        .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
        .await)
        .is_err()
    {
        return Ok(());
    }
    let member_uids = match rx.await {
        Ok(members) => members.into_iter().map(|(u, _)| u).collect::<Vec<_>>(),
        Err(_) => return Ok(()),
    };

    // Pre-fetch member data in one pass (InspIRCd/UnrealIRCd pattern: HasCapabilityFast)
    // This eliminates N×3 RwLock acquisitions per broadcast
    struct MemberCaps {
        has_multiline: bool,
        has_echo_message: bool,
    }
    let mut members: Vec<(String, MemberCaps)> = Vec::with_capacity(member_uids.len());
    for uid in member_uids {
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(&uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            members.push((
                uid,
                MemberCaps {
                    has_multiline: user.caps.contains("draft/multiline"),
                    has_echo_message: user.caps.contains("echo-message"),
                },
            ));
        } else {
            members.push((
                uid,
                MemberCaps {
                    has_multiline: false,
                    has_echo_message: false,
                },
            ));
        }
    }

    // Generate a unique batch reference, msgid, and server_time for outgoing
    // ALL recipients must receive the same msgid and time per IRCv3 spec
    let batch_ref = generate_batch_ref();
    let msgid = generate_msgid();
    let server_time = format_server_time();

    // Send to each member using pre-fetched capabilities
    for (member_uid, member_caps) in &members {
        // For the sender's own echo, get direct channel to bypass middleware and apply label manually
        // For other members, send directly to their sender channel
        if member_uid == ctx.uid {
            // Echo to self - use pre-fetched has_echo_message
            if member_caps.has_echo_message {
                let Some(sender_ref) = ctx.matrix.user_manager.senders.get(ctx.uid) else {
                    continue;
                };
                let sender = sender_ref.clone();
                drop(sender_ref);

                let sender_middleware = ResponseMiddleware::Direct(&sender);

                if member_caps.has_multiline {
                    send_multiline_batch(
                        &sender_middleware,
                        batch,
                        prefix,
                        &batch_ref,
                        &msgid,
                        cmd_type,
                        batch.response_label.as_deref(),
                    )
                    .await?;
                } else {
                    send_multiline_fallback(
                        &sender_middleware,
                        batch,
                        prefix,
                        &msgid,
                        &server_time,
                        cmd_type,
                    )
                    .await?;
                }
            }
        } else {
            // Send to other member - use direct channel and pre-fetched caps
            let Some(member_sender_ref) = ctx.matrix.user_manager.senders.get(member_uid) else {
                continue;
            };
            let member_sender = member_sender_ref.clone();
            drop(member_sender_ref);

            let member_middleware = ResponseMiddleware::Direct(&member_sender);

            if member_caps.has_multiline {
                send_multiline_batch(
                    &member_middleware,
                    batch,
                    prefix,
                    &batch_ref,
                    &msgid,
                    cmd_type,
                    None,
                )
                .await?;
            } else {
                send_multiline_fallback(
                    &member_middleware,
                    batch,
                    prefix,
                    &msgid,
                    &server_time,
                    cmd_type,
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Deliver a multiline batch to a single user.
async fn deliver_multiline_to_user(
    ctx: &mut Context<'_, RegisteredState>,
    batch: &BatchState,
    _combined: &str,
    prefix: &Prefix,
    cmd_type: &str,
    target_nick: &str,
) -> HandlerResult {
    // Find target user by nick
    let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "BATCH", target_nick).await? else {
        return Ok(());
    };

    // Get target's sender
    let Some(target_sender_ref) = ctx.matrix.user_manager.senders.get(&target_uid) else {
        return Ok(());
    };
    let target_sender = target_sender_ref.clone();
    drop(target_sender_ref);

    // Check if target has draft/multiline capability
    let target_user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(&target_uid)
        .map(|u| u.value().clone());
    let target_has_multiline = if let Some(target_user_arc) = target_user_arc {
        let user = target_user_arc.read().await;
        user.caps.contains("draft/multiline")
    } else {
        false
    };

    // Pre-fetch sender capabilities for echo (single read vs 2 reads)
    let sender_user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|u| u.value().clone());
    let (sender_has_echo, sender_has_multiline) = if let Some(sender_user_arc) = sender_user_arc {
        let user = sender_user_arc.read().await;
        (
            user.caps.contains("echo-message"),
            user.caps.contains("draft/multiline"),
        )
    } else {
        (false, false)
    };

    // Generate batch ref, msgid, and server_time (shared between target and echo)
    let batch_ref = generate_batch_ref();
    let msgid = generate_msgid();
    let server_time = format_server_time();

    let target_middleware = ResponseMiddleware::Direct(&target_sender);

    if target_has_multiline {
        send_multiline_batch(
            &target_middleware,
            batch,
            prefix,
            &batch_ref,
            &msgid,
            cmd_type,
            None,
        )
        .await?;
    } else {
        send_multiline_fallback(
            &target_middleware,
            batch,
            prefix,
            &msgid,
            &server_time,
            cmd_type,
        )
        .await?;
    }

    // Echo to sender if echo-message enabled (using pre-fetched caps)
    if sender_has_echo {
        // Get direct sender channel to bypass middleware and apply label manually
        let Some(sender_ref) = ctx.matrix.user_manager.senders.get(ctx.uid) else {
            return Ok(());
        };
        let sender = sender_ref.clone();
        drop(sender_ref);

        let sender_middleware = ResponseMiddleware::Direct(&sender);

        if sender_has_multiline {
            send_multiline_batch(
                &sender_middleware,
                batch,
                prefix,
                &batch_ref,
                &msgid,
                cmd_type,
                batch.response_label.as_deref(),
            )
            .await?;
        } else {
            send_multiline_fallback(
                &sender_middleware,
                batch,
                prefix,
                &msgid,
                &server_time,
                cmd_type,
            )
            .await?;
        }
    }

    Ok(())
}

/// Send a multiline batch (with BATCH +/-)
async fn send_multiline_batch(
    sender: &ResponseMiddleware<'_>,
    batch: &BatchState,
    prefix: &Prefix,
    batch_ref: &str,
    msgid: &str,
    cmd_type: &str,
    label: Option<&str>,
) -> HandlerResult {
    // Use provided msgid (shared across all recipients)
    let server_time = format_server_time();

    // Send BATCH +ref draft/multiline target
    // Start batch includes server-time and msgid
    let mut start_tags = vec![
        Tag::new("time", Some(server_time.clone())),
        Tag::new("msgid", Some(msgid.to_string())),
    ];

    // Add client-only tags from original BATCH + command
    for client_tag in &batch.client_tags {
        start_tags.push(client_tag.clone());
    }

    // Add label tag if present (for labeled-response)
    if let Some(lbl) = label {
        debug!("Adding label tag to BATCH: {}", lbl);
        start_tags.push(Tag::new("label", Some(lbl.to_string())));
    } else {
        debug!("No label to add to BATCH");
    }

    let start_batch = Message {
        tags: Some(start_tags),
        prefix: Some(prefix.clone()),
        command: Command::BATCH(
            format!("+{}", batch_ref),
            Some(BatchSubCommand::CUSTOM("draft/multiline".to_string())),
            Some(vec![batch.target.clone()]),
        ),
    };
    sender.send(start_batch).await?;

    // Send each line with batch=ref tag
    for line in &batch.lines {
        let mut tags = vec![Tag::new("batch", Some(batch_ref.to_string()))];
        if line.concat {
            tags.push(Tag::new("draft/multiline-concat", None));
        }

        let cmd = if cmd_type == "NOTICE" {
            Command::NOTICE(batch.target.clone(), line.content.clone())
        } else {
            Command::PRIVMSG(batch.target.clone(), line.content.clone())
        };

        let msg = Message {
            tags: Some(tags),
            prefix: Some(prefix.clone()),
            command: cmd,
        };
        sender.send(msg).await?;
    }

    // Send BATCH -ref
    let end_batch = Message {
        tags: None,
        prefix: None,
        command: Command::BATCH(format!("-{}", batch_ref), None, None),
    };
    sender.send(end_batch).await?;

    Ok(())
}

/// Send fallback individual lines to a client without draft/multiline.
async fn send_multiline_fallback(
    sender: &ResponseMiddleware<'_>,
    batch: &BatchState,
    prefix: &Prefix,
    msgid: &str,
    server_time: &str,
    cmd_type: &str,
) -> HandlerResult {
    // Use provided msgid and server-time (shared across all recipients)

    // For fallback: send each non-empty line as a separate message
    // Ignore concat tags (client can't handle multiline anyway)
    // Skip empty lines per spec
    let mut message_index = 0;

    for line in &batch.lines {
        // Skip empty lines in fallback
        if line.content.is_empty() {
            continue;
        }

        let cmd = if cmd_type == "NOTICE" {
            Command::NOTICE(batch.target.clone(), line.content.clone())
        } else {
            Command::PRIVMSG(batch.target.clone(), line.content.clone())
        };

        // First non-empty line gets msgid, server-time, and client tags
        // All subsequent lines get server-time and client tags (NO msgid)
        let mut tags = vec![Tag::new("time", Some(server_time.to_string()))];

        // Add client-only tags from original BATCH + command to ALL messages
        for client_tag in &batch.client_tags {
            tags.push(client_tag.clone());
        }

        // Only first message gets msgid
        if message_index == 0 {
            tags.insert(1, Tag::new("msgid", Some(msgid.to_string())));
        }

        let msg = Message {
            tags: Some(tags),
            prefix: Some(prefix.clone()),
            command: cmd,
        };
        sender.send(msg).await?;

        message_index += 1;
    }

    Ok(())
}

/// Send a FAIL message for batch errors.
async fn send_fail<S>(ctx: &mut Context<'_, S>, code: &str, message: &str) -> HandlerResult {
    let reply = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::Raw(
            "FAIL".to_string(),
            vec![
                "BATCH".to_string(),
                code.to_string(),
                format!(":{}", message),
            ],
        ),
    };
    ctx.sender.send(reply).await?;
    Ok(())
}
