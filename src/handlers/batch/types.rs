//! Type definitions for batch handling.

use slirc_proto::Tag;

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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Constants tests ==========

    #[test]
    fn multiline_max_bytes_is_4096() {
        assert_eq!(MULTILINE_MAX_BYTES, 4096);
    }

    #[test]
    fn multiline_max_lines_is_32() {
        assert_eq!(MULTILINE_MAX_LINES, 32);
    }

    // ========== BatchState construction tests ==========

    #[test]
    fn batch_state_can_be_constructed() {
        let state = BatchState {
            batch_type: "draft/multiline".to_string(),
            target: "#channel".to_string(),
            lines: Vec::new(),
            total_bytes: 0,
            command_type: Some("PRIVMSG".to_string()),
            response_label: None,
            client_tags: Vec::new(),
        };
        assert_eq!(state.batch_type, "draft/multiline");
        assert_eq!(state.target, "#channel");
        assert!(state.lines.is_empty());
        assert_eq!(state.total_bytes, 0);
        assert_eq!(state.command_type, Some("PRIVMSG".to_string()));
        assert!(state.response_label.is_none());
        assert!(state.client_tags.is_empty());
    }

    #[test]
    fn batch_state_with_lines() {
        let mut state = BatchState {
            batch_type: "draft/multiline".to_string(),
            target: "nick".to_string(),
            lines: Vec::new(),
            total_bytes: 0,
            command_type: Some("NOTICE".to_string()),
            response_label: Some("abc123".to_string()),
            client_tags: Vec::new(),
        };
        state.lines.push(BatchLine {
            content: "Hello".to_string(),
            concat: false,
        });
        state.lines.push(BatchLine {
            content: "World".to_string(),
            concat: true,
        });
        state.total_bytes = 10;

        assert_eq!(state.lines.len(), 2);
        assert_eq!(state.total_bytes, 10);
        assert_eq!(state.response_label, Some("abc123".to_string()));
    }

    // ========== BatchLine construction tests ==========

    #[test]
    fn batch_line_can_be_constructed() {
        let line = BatchLine {
            content: "Hello, world!".to_string(),
            concat: false,
        };
        assert_eq!(line.content, "Hello, world!");
        assert!(!line.concat);
    }

    #[test]
    fn batch_line_concat_true() {
        let line = BatchLine {
            content: "continued".to_string(),
            concat: true,
        };
        assert_eq!(line.content, "continued");
        assert!(line.concat);
    }

    #[test]
    fn batch_line_empty_content() {
        let line = BatchLine {
            content: String::new(),
            concat: false,
        };
        assert!(line.content.is_empty());
        assert!(!line.concat);
    }
}
