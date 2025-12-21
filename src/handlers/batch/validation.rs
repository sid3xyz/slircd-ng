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
