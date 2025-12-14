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
