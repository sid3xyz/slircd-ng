//! Message processing logic for batch handling.

use super::types::BatchLine;
use super::validation::{
    validate_command_type, validate_concat_content, validate_content_size, validate_line_count,
};
use crate::state::{BatchRouting, SessionState};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::batch::types::{BatchLine, BatchState, MULTILINE_MAX_LINES};
    use crate::state::SessionState;
    use crate::state::client::DeviceId;
    use crate::state::session::ReattachInfo;
    use slirc_proto::MessageRef;
    use std::collections::HashSet;

    // --- Mock Session State ---

    struct MockSessionState {
        batch: Option<BatchState>,
        batch_ref: Option<String>,
        is_server_conn: bool,
        capabilities: HashSet<String>,
    }

    impl MockSessionState {
        fn new() -> Self {
            Self {
                batch: None,
                batch_ref: None,
                is_server_conn: false,
                capabilities: HashSet::new(),
            }
        }

        fn with_batch(mut self, ref_tag: &str, target: &str) -> Self {
            self.batch_ref = Some(ref_tag.to_string());
            self.batch = Some(BatchState {
                batch_type: "draft/multiline".to_string(),
                target: target.to_string(),
                lines: Vec::new(),
                total_bytes: 0,
                command_type: None,
                response_label: None,
                client_tags: Vec::new(),
            });
            self
        }
    }

    impl SessionState for MockSessionState {
        fn nick(&self) -> Option<&str> {
            Some("Tester")
        }
        fn set_nick(&mut self, _nick: String) {}
        fn is_registered(&self) -> bool {
            true
        }
        fn set_device_id(&mut self, _device_id: Option<DeviceId>) {}
        fn set_reattach_info(&mut self, _reattach_info: Option<ReattachInfo>) {}
        fn capabilities(&self) -> &HashSet<String> {
            &self.capabilities
        }
        fn capabilities_mut(&mut self) -> &mut HashSet<String> {
            &mut self.capabilities
        }
        fn set_cap_negotiating(&mut self, _negotiating: bool) {}
        fn set_cap_version(&mut self, _version: u32) {}
        fn is_tls(&self) -> bool {
            false
        }
        fn certfp(&self) -> Option<&str> {
            None
        }

        fn active_batch_mut(&mut self) -> &mut Option<BatchState> {
            &mut self.batch
        }

        fn active_batch_ref(&self) -> Option<&str> {
            self.batch_ref.as_deref()
        }

        fn is_server(&self) -> bool {
            self.is_server_conn
        }
    }

    // --- Tests ---

    #[test]
    fn test_process_normal_message() {
        let mut state = MockSessionState::new();
        let raw = "PRIVMSG #test :Hello world";
        let msg = MessageRef::parse(raw).unwrap();

        let result = process_batch_message(&mut state, &msg, "test.server");

        // Should return Ok(None) meaning "not consumed by batch"
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_process_batch_accumulation() {
        let mut state = MockSessionState::new().with_batch("123", "#test");
        let raw = "@batch=123 PRIVMSG #test :Line 1";
        let msg = MessageRef::parse(raw).unwrap();

        let result = process_batch_message(&mut state, &msg, "test.server");

        // Should return Ok(Some("123")) meaning consumed
        assert_eq!(result.unwrap(), Some("123".to_string()));

        // Verify state
        let batch = state.batch.unwrap();
        assert_eq!(batch.lines.len(), 1);
        assert_eq!(batch.lines[0].content, "Line 1");
    }

    #[test]
    fn test_batch_tag_mismatch() {
        let mut state = MockSessionState::new().with_batch("123", "#test");
        let raw = "@batch=456 PRIVMSG #test :Line 1";
        let msg = MessageRef::parse(raw).unwrap();

        let result = process_batch_message(&mut state, &msg, "test.server");

        // Should fail due to mismatch
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("mismatch"));
    }

    #[test]
    fn test_batch_limit_enforcement() {
        // Create a batch that is almost full
        let mut state = MockSessionState::new().with_batch("123", "#test");

        // Fill up to MULTILINE_MAX_LINES - 1
        for i in 0..(MULTILINE_MAX_LINES - 1) {
            state.batch.as_mut().unwrap().lines.push(BatchLine {
                content: format!("Line {}", i),
                concat: false,
            });
        }

        // Add the last allowed line
        let raw = "@batch=123 PRIVMSG #test :Last Line";
        let msg = MessageRef::parse(raw).unwrap();
        assert!(process_batch_message(&mut state, &msg, "test.server").is_ok());

        // Try to add one more
        let raw_overflow = "@batch=123 PRIVMSG #test :Overflow";
        let msg_overflow = MessageRef::parse(raw_overflow).unwrap();
        let result = process_batch_message(&mut state, &msg_overflow, "test.server");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("MULTILINE_MAX_LINES"));
    }
}
