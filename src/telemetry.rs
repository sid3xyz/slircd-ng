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
