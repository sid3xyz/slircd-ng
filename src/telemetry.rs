//! Telemetry utilities for command timing and message correlation.

use std::time::Instant;

/// Guard for timing command execution and recording metrics.
///
/// Records command latency when dropped.
pub struct CommandTimer {
    command: String,
    start: Instant,
}

impl CommandTimer {
    /// Start timing a command.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            start: Instant::now(),
        }
    }
}

impl Drop for CommandTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        crate::metrics::record_command(&self.command, duration);
    }
}

/// Extract msgid from message tags if present.
pub fn extract_msgid(msg: &slirc_proto::MessageRef<'_>) -> Option<String> {
    msg.tags_iter()
        .find(|(k, _)| *k == "msgid")
        .map(|(_, v)| v.to_string())
}

/// Standardized span constructors for IRC observability.
#[allow(dead_code)]
pub mod spans {
    use tracing::{Span, info_span};

    /// Create a span for a client connection.
    pub fn connection(uid: &str, ip: &str) -> Span {
        info_span!("connection", uid = %uid, ip = %ip)
    }

    /// Create a span for a server connection.
    pub fn peer(sid: &str, name: &str) -> Span {
        info_span!("peer", sid = %sid, name = %name)
    }

    /// Create a span for a command execution.
    pub fn command(name: &str, source: &str, target: Option<&str>) -> Span {
        if let Some(target) = target {
            info_span!("command", name = %name, source = %source, target = %target)
        } else {
            info_span!("command", name = %name, source = %source)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::MessageRef;

    #[test]
    fn test_extract_msgid_present() {
        // Message with msgid tag
        let raw = "@msgid=abc123 :nick!user@host PRIVMSG #chan :Hello\r\n";
        let msg = MessageRef::parse(raw).unwrap();
        let result = extract_msgid(&msg);
        assert_eq!(result, Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_msgid_absent() {
        // Message without msgid tag
        let raw = ":nick!user@host PRIVMSG #chan :Hello\r\n";
        let msg = MessageRef::parse(raw).unwrap();
        let result = extract_msgid(&msg);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_msgid_other_tags() {
        // Message with other tags but no msgid
        let raw =
            "@time=2025-01-01T00:00:00Z;account=testuser :nick!user@host PRIVMSG #chan :Hello\r\n";
        let msg = MessageRef::parse(raw).unwrap();
        let result = extract_msgid(&msg);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_msgid_with_multiple_tags() {
        // Message with msgid among multiple tags
        let raw = "@time=2025-01-01T00:00:00Z;msgid=xyz789;account=testuser :nick!user@host PRIVMSG #chan :Hello\r\n";
        let msg = MessageRef::parse(raw).unwrap();
        let result = extract_msgid(&msg);
        assert_eq!(result, Some("xyz789".to_string()));
    }

    #[test]
    fn test_command_timer_creation() {
        // Just verify we can create a CommandTimer without panicking
        let timer = CommandTimer::new("PRIVMSG");
        assert_eq!(timer.command, "PRIVMSG");
    }
}
