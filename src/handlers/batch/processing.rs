//! Message processing logic for batch handling.

use super::types::BatchLine;
use super::validation::{
    validate_command_type, validate_concat_content, validate_content_size, validate_line_count,
};
use crate::state::{SessionState, BatchRouting};
use slirc_proto::MessageRef;

/// Process a message within an active batch.
///
/// This is called from the connection loop when a message has a `batch` tag.
/// Returns `Ok(Some(batch_ref))` if the message was consumed by the batch,
/// `Ok(None)` if it should be dispatched normally, or `Err` with a FAIL message.
///
/// The BATCH handler processes batch start/end commands, but messages within
/// a batch (PRIVMSG/NOTICE with batch=ref tag) are intercepted here before
/// normal dispatch and accumulated in the session state's active_batch.
pub fn process_batch_message<S: SessionState>(
    state: &mut S,
    msg: &MessageRef<'_>,
    _server_name: &str,
) -> Result<Option<String>, String> {
    // Check if message has a batch tag
    let batch_ref = msg.tag_value("batch");

    // If no batch tag, process normally
    let Some(batch_ref) = batch_ref else {
        return Ok(None);
    };

    // Check if it matches our active batch
    let active_ref = match state.active_batch_ref() {
        Some(r) => r.to_string(),
        None => return Ok(None), // No active batch, process normally
    };

    if batch_ref != active_ref {
        return Err(format!(
            "FAIL BATCH MULTILINE_INVALID :Batch tag mismatch (expected {}, got {})",
            active_ref, batch_ref
        ));
    }

    let is_server = state.is_server();

    // Check routing first to avoid borrow conflicts
    let is_local_routing = matches!(state.batch_routing(), Some(BatchRouting::Local(_)));

    // For Local routing (chathistory), we also stream to the user immediately
    // instead of buffering in the server state.
    if is_local_routing {
        return Ok(None);
    }

    // Add to the active batch
    let batch = match state.active_batch_mut() {
        Some(b) => b,
        None => return Ok(None),
    };
    // For NETSPLIT batches (server-side), we stream messages instead of buffering.
    // This prevents memory spikes during large netsplits.
    if batch.batch_type == "NETSPLIT" {
        return Ok(None);
    }

    // Check command type (must be PRIVMSG or NOTICE)
    // Exception: Server batches can contain other commands (e.g. QUIT in NETSPLIT, though handled above)
    let cmd_name = msg.command_name().to_ascii_uppercase();
    if !is_server && cmd_name != "PRIVMSG" && cmd_name != "NOTICE" {
        return Err(format!(
            "FAIL BATCH MULTILINE_INVALID :Invalid command {} in multiline batch",
            cmd_name
        ));
    }

    // Verify command type consistency
    if !is_server {
        validate_command_type(batch, &cmd_name)?;
    }

    // Set command type if not already set
    if batch.command_type.is_none() {
        batch.command_type = Some(cmd_name);
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
    validate_concat_content(has_concat, content)?;

    // Check limits
    let new_bytes = validate_content_size(batch, content.len(), has_concat)?;
    validate_line_count(batch)?;

    // Add the line
    batch.total_bytes = new_bytes;
    batch.lines.push(BatchLine {
        content: content.to_string(),
        concat: has_concat,
    });

    // Return the batch ref to indicate this message was consumed
    Ok(Some(batch_ref.to_string()))
}
