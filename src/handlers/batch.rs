//! BATCH command handler for IRCv3 message batching.
//!
//! Handles client-to-server batches, particularly draft/multiline for
//! sending multi-line messages.
//!
//! Reference: <https://ircv3.net/specs/extensions/batch>
//! Reference: <https://ircv3.net/specs/extensions/multiline>

use super::{Context, Handler, HandlerResult, ResponseMiddleware};
use async_trait::async_trait;
use slirc_proto::{
    format_server_time, generate_batch_ref, generate_msgid, BatchSubCommand, Command, Message,
    MessageRef, Prefix, Tag,
};
use tracing::debug;

/// Maximum bytes allowed in a multiline batch message.
/// Per Ergo's implementation and irctest expectations.
pub const MULTILINE_MAX_BYTES: usize = 4096;
/// Maximum lines allowed in a multiline batch.
/// Per Ergo's implementation and irctest expectations.
pub const MULTILINE_MAX_LINES: usize = 32;

/// State for an in-progress batch.
#[derive(Debug, Clone)]
pub struct BatchState {
    /// Batch type (e.g., "draft/multiline").
    pub batch_type: String,
    /// Target for the batch (e.g., channel or nick for multiline).
    pub target: String,
    /// Accumulated message lines.
    pub lines: Vec<BatchLine>,
    /// Total bytes accumulated (just the message content).
    pub total_bytes: usize,
    /// Command type (PRIVMSG or NOTICE).
    pub command_type: Option<String>,
    /// Response label from labeled-response (saved from BATCH +, applied to BATCH -).
    pub response_label: Option<String>,
    /// Client-only tags from BATCH + command (tags starting with '+').
    pub client_tags: Vec<Tag>,
}
/// A line within a batch.
#[derive(Debug, Clone)]
pub struct BatchLine {
    /// The message content.
    pub content: String,
    /// Whether this line should be concatenated with the previous (no newline).
    pub concat: bool,
}

/// Handler for BATCH command.
pub struct BatchHandler;

#[async_trait]
impl Handler for BatchHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // BATCH +ref type [params...] or BATCH -ref
        let ref_tag = msg.arg(0).unwrap_or("");

        if ref_tag.is_empty() {
            return Ok(());
        }

        let nick = ctx
            .handshake
            .nick
            .clone()
            .unwrap_or_else(|| "*".to_string());
        let server_name = ctx.matrix.server_info.name.clone();

        if let Some(stripped) = ref_tag.strip_prefix('+') {
            // Start a new batch
            let batch_type = msg.arg(1).unwrap_or("");
            let target = msg.arg(2).unwrap_or("");

            if batch_type.eq_ignore_ascii_case("draft/multiline") {
                // Check if client has the capability
                if !ctx.handshake.capabilities.contains("draft/multiline") {
                    send_fail(ctx, &server_name, "MULTILINE_INVALID", "Capability not negotiated").await?;
                    return Ok(());
                }

                if target.is_empty() {
                    send_fail(ctx, &server_name, "MULTILINE_INVALID", "No target specified").await?;
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
                ctx.handshake.active_batch = Some(BatchState {
                    batch_type: "draft/multiline".to_string(),
                    target: target.to_string(),
                    lines: Vec::new(),
                    total_bytes: 0,
                    command_type: None,
                    response_label,
                    client_tags,
                });
                ctx.handshake.active_batch_ref = Some(stripped.to_string());

                // CRITICAL: Suppress the automatic labeled-response ACK for BATCH +
                // The label will be applied manually to the BATCH echo when BATCH - is processed
                ctx.suppress_labeled_ack = true;
            }
        } else if let Some(stripped) = ref_tag.strip_prefix('-') {
            // End a batch
            if let Some(ref active_ref) = ctx.handshake.active_batch_ref {
                if active_ref != stripped {
                    send_fail(ctx, &server_name, "MULTILINE_INVALID", "Batch reference mismatch").await?;
                    return Ok(());
                }
            } else {
                // No active batch
                return Ok(());
            }

            // Process the completed batch
            if let Some(batch) = ctx.handshake.active_batch.take() {
                ctx.handshake.active_batch_ref = None;

                if batch.batch_type == "draft/multiline" {
                    process_multiline_batch(ctx, &batch, &nick, &server_name).await?;
                }
            }
        }

        Ok(())
    }
}

/// Process a message within an active batch.
///
/// This is called from the connection loop when a message has a `batch` tag.
/// Returns `Ok(Some(batch_ref))` if the message was consumed by the batch,
/// `Ok(None)` if it should be dispatched normally, or `Err` with a FAIL message.
///
/// The BATCH handler processes batch start/end commands, but messages within
/// a batch (PRIVMSG/NOTICE with batch=ref tag) are intercepted here before
/// normal dispatch and accumulated in `HandshakeState.active_batch`.
pub fn process_batch_message(
    handshake: &mut super::HandshakeState,
    msg: &MessageRef<'_>,
    _server_name: &str,
) -> Result<Option<String>, String> {
    // Check if message has a batch tag
    let batch_ref = msg.tag_value("batch");

    if batch_ref.is_none() {
        return Ok(None);
    }

    let batch_ref = batch_ref.unwrap();

    // Check if it matches our active batch
    let active_ref = match &handshake.active_batch_ref {
        Some(r) => r.clone(),
        None => return Ok(None), // No active batch, process normally
    };

    if batch_ref != active_ref {
        return Err(format!(
            "FAIL BATCH MULTILINE_INVALID :Batch tag mismatch (expected {}, got {})",
            active_ref, batch_ref
        ));
    }

    // Add to the active batch
    let batch = match &mut handshake.active_batch {
        Some(b) => b,
        None => return Ok(None),
    };

    // Check command type (must be PRIVMSG or NOTICE)
    let cmd_name = msg.command_name().to_ascii_uppercase();
    if cmd_name != "PRIVMSG" && cmd_name != "NOTICE" {
        return Err(format!("FAIL BATCH MULTILINE_INVALID :Invalid command {} in multiline batch", cmd_name));
    }

    // Verify command type consistency
    if let Some(ref existing_type) = batch.command_type {
        if existing_type != &cmd_name {
            return Err("FAIL BATCH MULTILINE_INVALID :Cannot mix PRIVMSG and NOTICE in multiline batch".to_string());
        }
    } else {
        batch.command_type = Some(cmd_name.clone());
    }

    // Verify target matches
    let msg_target = msg.arg(0).unwrap_or("");
    if !msg_target.eq_ignore_ascii_case(&batch.target) {
        return Err(format!(
            "FAIL BATCH MULTILINE_INVALID_TARGET {} {} :Mismatched target in multiline batch",
            batch.target, msg_target
        ));
    }

    // Get message content
    let content = msg.arg(1).unwrap_or("");

    // Check for concat tag
    let has_concat = msg.tag_value("draft/multiline-concat").is_some();

    // Validate: concat lines must not be blank
    if has_concat && content.is_empty() {
        return Err("FAIL BATCH MULTILINE_INVALID :Cannot concatenate blank line".to_string());
    }

    // Check limits
    let new_bytes = batch.total_bytes + content.len() + if batch.lines.is_empty() || has_concat { 0 } else { 1 };
    if new_bytes > MULTILINE_MAX_BYTES {
        return Err(format!(
            "FAIL BATCH MULTILINE_MAX_BYTES {} :Multiline batch max-bytes exceeded",
            MULTILINE_MAX_BYTES
        ));
    }

    if batch.lines.len() >= MULTILINE_MAX_LINES {
        return Err(format!(
            "FAIL BATCH MULTILINE_MAX_LINES {} :Multiline batch max-lines exceeded",
            MULTILINE_MAX_LINES
        ));
    }

    // Add the line
    batch.total_bytes = new_bytes;
    batch.lines.push(BatchLine {
        content: content.to_string(),
        concat: has_concat,
    });

    // Return the batch ref to indicate this message was consumed
    Ok(Some(batch_ref.to_string()))
}

/// Process a completed multiline batch by delivering to recipients.
async fn process_multiline_batch(
    ctx: &mut Context<'_>,
    batch: &BatchState,
    nick: &str,
    server_name: &str,
) -> HandlerResult {
    // Validate batch isn't empty or all blank
    if batch.lines.is_empty() {
        send_fail(ctx, server_name, "MULTILINE_INVALID", "Empty multiline batch").await?;
        return Ok(());
    }

    let all_blank = batch.lines.iter().all(|l| l.content.is_empty());
    if all_blank {
        send_fail(ctx, server_name, "MULTILINE_INVALID", "Multiline batch with blank lines only").await?;
        return Ok(());
    }

    // Build the combined message
    let mut combined = String::new();
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
    let prefix = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        Prefix::Nickname(
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
        )
    } else {
        Prefix::Nickname(nick.to_string(), "user".to_string(), "host".to_string())
    };

    // Determine if target is a channel or user
    let is_channel = target.starts_with('#') || target.starts_with('&');

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
    ctx: &mut Context<'_>,
    batch: &BatchState,
    _combined: &str,
    prefix: &Prefix,
    cmd_type: &str,
) -> HandlerResult {
    let channel_lower = batch.target.to_lowercase();

    let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) else {
        // Channel doesn't exist - send error
        let reply = super::server_reply(
            &ctx.matrix.server_info.name,
            slirc_proto::Response::ERR_NOSUCHCHANNEL,
            vec![
                ctx.handshake.nick.clone().unwrap_or_default(),
                batch.target.clone(),
                "No such channel".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(());
    };

    let channel = channel_ref.read().await;

    // Get list of members and their UIDs
    let members: Vec<(String, String)> = channel
        .members
        .keys()
        .map(|uid| {
            let nick = ctx
                .matrix
                .users
                .get(uid)
                .map(|u| {
                    // Can't await here, so we need to use try_read
                    if let Ok(guard) = u.try_read() {
                        guard.nick.clone()
                    } else {
                        uid.clone()
                    }
                })
                .unwrap_or_else(|| uid.clone());
            (uid.clone(), nick)
        })
        .collect();

    drop(channel);

    // Generate a unique batch reference, msgid, and server_time for outgoing
    // ALL recipients must receive the same msgid and time per IRCv3 spec
    let batch_ref = generate_batch_ref();
    let msgid = generate_msgid();
    let server_time = format_server_time();

    // Send to each member
    for (member_uid, _member_nick) in &members {
        // Check if member has draft/multiline capability
        let has_multiline = if let Some(user_ref) = ctx.matrix.users.get(member_uid) {
            let user = user_ref.read().await;
            user.caps.contains("draft/multiline")
        } else {
            false
        };

        // For the sender's own echo, get direct channel to bypass middleware and apply label manually
        // For other members, send directly to their sender channel
        if member_uid == ctx.uid {
            // Echo to self - get direct sender channel and apply label manually
            if ctx
                .matrix
                .users
                .get(ctx.uid)
                .is_some_and(|u| u.try_read().is_ok_and(|g| g.caps.contains("echo-message")))
            {
                let Some(sender_ref) = ctx.matrix.senders.get(ctx.uid) else {
                    continue;
                };
                let sender = sender_ref.clone();
                drop(sender_ref);

                let sender_middleware = ResponseMiddleware::Direct(&sender);

                if has_multiline {
                    send_multiline_batch(&sender_middleware, batch, prefix, &batch_ref, &msgid, cmd_type, batch.response_label.as_deref()).await?;
                } else {
                    send_multiline_fallback(&sender_middleware, batch, prefix, &msgid, &server_time, cmd_type).await?;
                }
            }
        } else {
            // Send to other member - use direct channel
            let Some(member_sender_ref) = ctx.matrix.senders.get(member_uid) else {
                continue;
            };
            let member_sender = member_sender_ref.clone();
            drop(member_sender_ref);

            let member_middleware = ResponseMiddleware::Direct(&member_sender);

            if has_multiline {
                send_multiline_batch(&member_middleware, batch, prefix, &batch_ref, &msgid, cmd_type, None).await?;
            } else {
                send_multiline_fallback(&member_middleware, batch, prefix, &msgid, &server_time, cmd_type).await?;
            }
        }
    }

    Ok(())
}

/// Deliver a multiline batch to a single user.
async fn deliver_multiline_to_user(
    ctx: &mut Context<'_>,
    batch: &BatchState,
    _combined: &str,
    prefix: &Prefix,
    cmd_type: &str,
    target_nick: &str,
) -> HandlerResult {
    // Find target user by nick
    let target_uid = ctx.matrix.nicks.get(&target_nick.to_lowercase()).map(|r| r.clone());

    let Some(target_uid) = target_uid else {
        // User not found
        let reply = super::server_reply(
            &ctx.matrix.server_info.name,
            slirc_proto::Response::ERR_NOSUCHNICK,
            vec![
                ctx.handshake.nick.clone().unwrap_or_default(),
                target_nick.to_string(),
                "No such nick".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;
        return Ok(());
    };

    // Get target's sender
    let Some(target_sender_ref) = ctx.matrix.senders.get(&target_uid) else {
        return Ok(());
    };
    let target_sender = target_sender_ref.clone();
    drop(target_sender_ref);

    // Check if target has draft/multiline capability
    let has_multiline = if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
        let user = user_ref.read().await;
        user.caps.contains("draft/multiline")
    } else {
        false
    };

    // Generate batch ref, msgid, and server_time (shared between target and echo)
    let batch_ref = generate_batch_ref();
    let msgid = generate_msgid();
    let server_time = format_server_time();

    let target_middleware = ResponseMiddleware::Direct(&target_sender);

    if has_multiline {
        send_multiline_batch(&target_middleware, batch, prefix, &batch_ref, &msgid, cmd_type, None).await?;
    } else {
        send_multiline_fallback(&target_middleware, batch, prefix, &msgid, &server_time, cmd_type).await?;
    }

    // Echo to sender if echo-message enabled
    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        if user.caps.contains("echo-message") {
            drop(user);
            let sender_has_multiline = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                user.caps.contains("draft/multiline")
            } else {
                false
            };

            // Get direct sender channel to bypass middleware and apply label manually
            let Some(sender_ref) = ctx.matrix.senders.get(ctx.uid) else {
                return Ok(());
            };
            let sender = sender_ref.clone();
            drop(sender_ref);

            let sender_middleware = ResponseMiddleware::Direct(&sender);

            if sender_has_multiline {
                send_multiline_batch(&sender_middleware, batch, prefix, &batch_ref, &msgid, cmd_type, batch.response_label.as_deref()).await?;
            } else {
                send_multiline_fallback(&sender_middleware, batch, prefix, &msgid, &server_time, cmd_type).await?;
            }
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
    sender: &crate::handlers::ResponseMiddleware<'_>,
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
async fn send_fail(
    ctx: &mut Context<'_>,
    server_name: &str,
    code: &str,
    message: &str,
) -> HandlerResult {
    let reply = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::Raw(
            "FAIL".to_string(),
            vec!["BATCH".to_string(), code.to_string(), format!(":{}", message)],
        ),
    };
    ctx.sender.send(reply).await?;
    Ok(())
}
