//! Validation functions for multiline batch content.

use super::types::{BatchState, MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES};

/// Validate that content length doesn't exceed limits.
pub(super) fn validate_content_size(
    batch: &BatchState,
    new_content_len: usize,
    has_concat: bool,
) -> Result<usize, String> {
    let new_bytes = batch.total_bytes
        + new_content_len
        + if batch.lines.is_empty() || has_concat {
            0
        } else {
            1
        };

    if new_bytes > MULTILINE_MAX_BYTES {
        return Err(format!(
            "FAIL BATCH MULTILINE_MAX_BYTES {} :Multiline batch max-bytes exceeded",
            MULTILINE_MAX_BYTES
        ));
    }

    Ok(new_bytes)
}

/// Validate that line count doesn't exceed limits.
pub(super) fn validate_line_count(batch: &BatchState) -> Result<(), String> {
    if batch.lines.len() >= MULTILINE_MAX_LINES {
        return Err(format!(
            "FAIL BATCH MULTILINE_MAX_LINES {} :Multiline batch max-lines exceeded",
            MULTILINE_MAX_LINES
        ));
    }

    Ok(())
}

/// Validate that concat lines are not blank.
pub(super) fn validate_concat_content(has_concat: bool, content: &str) -> Result<(), String> {
    if has_concat && content.is_empty() {
        return Err("FAIL BATCH MULTILINE_INVALID :Cannot concatenate blank line".to_string());
    }

    Ok(())
}

/// Validate command type consistency within batch.
pub(super) fn validate_command_type(batch: &BatchState, cmd_name: &str) -> Result<(), String> {
    if let Some(ref existing_type) = batch.command_type
        && existing_type != cmd_name
    {
        return Err(
            "FAIL BATCH MULTILINE_INVALID :Cannot mix PRIVMSG and NOTICE in multiline batch"
                .to_string(),
        );
    }

    Ok(())
}

/// Validate that batch is not empty or all blank.
pub(super) fn validate_batch_not_empty(batch: &BatchState) -> Result<(), &'static str> {
    if batch.lines.is_empty() {
        return Err("Empty multiline batch");
    }

    let all_blank = batch.lines.iter().all(|l| l.content.is_empty());
    if all_blank {
        return Err("Multiline batch with blank lines only");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::batch::types::BatchLine;

    /// Create an empty batch state for testing.
    fn empty_batch() -> BatchState {
        BatchState {
            batch_type: "draft/multiline".to_string(),
            target: "#test".to_string(),
            lines: vec![],
            total_bytes: 0,
            command_type: None,
            response_label: None,
            client_tags: vec![],
        }
    }

    /// Create a batch with existing content for testing.
    fn batch_with_bytes(bytes: usize, line_count: usize) -> BatchState {
        let lines: Vec<BatchLine> = (0..line_count)
            .map(|_| BatchLine {
                content: "x".to_string(),
                concat: false,
            })
            .collect();
        BatchState {
            batch_type: "draft/multiline".to_string(),
            target: "#test".to_string(),
            lines,
            total_bytes: bytes,
            command_type: None,
            response_label: None,
            client_tags: vec![],
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_content_size tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_content_size_under_limit() {
        let batch = empty_batch();
        let result = validate_content_size(&batch, 100, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100);
    }

    #[test]
    fn test_content_size_at_limit() {
        let batch = empty_batch();
        let result = validate_content_size(&batch, MULTILINE_MAX_BYTES, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), MULTILINE_MAX_BYTES);
    }

    #[test]
    fn test_content_size_over_limit() {
        let batch = empty_batch();
        let result = validate_content_size(&batch, MULTILINE_MAX_BYTES + 1, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("MULTILINE_MAX_BYTES"));
    }

    #[test]
    fn test_content_size_accumulates() {
        // Existing batch with 1000 bytes, adding 500 more + 1 for newline separator
        let batch = batch_with_bytes(1000, 1);
        let result = validate_content_size(&batch, 500, false);
        assert!(result.is_ok());
        // 1000 + 500 + 1 (separator) = 1501
        assert_eq!(result.unwrap(), 1501);
    }

    #[test]
    fn test_content_size_with_concat_no_separator() {
        // With concat=true, no separator byte is added
        let batch = batch_with_bytes(1000, 1);
        let result = validate_content_size(&batch, 500, true);
        assert!(result.is_ok());
        // 1000 + 500 + 0 (no separator for concat) = 1500
        assert_eq!(result.unwrap(), 1500);
    }

    #[test]
    fn test_content_size_first_line_no_separator() {
        // First line (empty batch) has no separator
        let batch = empty_batch();
        let result = validate_content_size(&batch, 500, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 500);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_line_count tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_line_count_under_limit() {
        let batch = batch_with_bytes(100, 10);
        let result = validate_line_count(&batch);
        assert!(result.is_ok());
    }

    #[test]
    fn test_line_count_at_limit() {
        // At limit means we can't add more
        let batch = batch_with_bytes(100, MULTILINE_MAX_LINES);
        let result = validate_line_count(&batch);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("MULTILINE_MAX_LINES"));
    }

    #[test]
    fn test_line_count_one_below_limit() {
        let batch = batch_with_bytes(100, MULTILINE_MAX_LINES - 1);
        let result = validate_line_count(&batch);
        assert!(result.is_ok());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_concat_content tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_concat_with_content() {
        let result = validate_concat_content(true, "some text");
        assert!(result.is_ok());
    }

    #[test]
    fn test_concat_with_empty_fails() {
        let result = validate_concat_content(true, "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("MULTILINE_INVALID"));
    }

    #[test]
    fn test_no_concat_with_empty_ok() {
        // Without concat flag, empty content is fine
        let result = validate_concat_content(false, "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_concat_with_content_ok() {
        let result = validate_concat_content(false, "some text");
        assert!(result.is_ok());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_command_type tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_command_type_none_accepts_any() {
        let batch = empty_batch();
        let result = validate_command_type(&batch, "PRIVMSG");
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_type_match() {
        let mut batch = empty_batch();
        batch.command_type = Some("PRIVMSG".to_string());
        let result = validate_command_type(&batch, "PRIVMSG");
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_type_mismatch() {
        let mut batch = empty_batch();
        batch.command_type = Some("PRIVMSG".to_string());
        let result = validate_command_type(&batch, "NOTICE");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("mix PRIVMSG and NOTICE"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // validate_batch_not_empty tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_batch_not_empty_ok() {
        let mut batch = empty_batch();
        batch.lines.push(BatchLine {
            content: "hello".to_string(),
            concat: false,
        });
        let result = validate_batch_not_empty(&batch);
        assert!(result.is_ok());
    }

    #[test]
    fn test_batch_empty_fails() {
        let batch = empty_batch();
        let result = validate_batch_not_empty(&batch);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Empty multiline batch");
    }

    #[test]
    fn test_batch_all_blank_fails() {
        let mut batch = empty_batch();
        batch.lines.push(BatchLine {
            content: "".to_string(),
            concat: false,
        });
        batch.lines.push(BatchLine {
            content: "".to_string(),
            concat: false,
        });
        let result = validate_batch_not_empty(&batch);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Multiline batch with blank lines only");
    }

    #[test]
    fn test_batch_mixed_content_ok() {
        // Some blank, some with content → should be OK
        let mut batch = empty_batch();
        batch.lines.push(BatchLine {
            content: "".to_string(),
            concat: false,
        });
        batch.lines.push(BatchLine {
            content: "hello".to_string(),
            concat: false,
        });
        let result = validate_batch_not_empty(&batch);
        assert!(result.is_ok());
    }
}
