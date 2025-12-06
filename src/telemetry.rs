//! IRC-Aware Telemetry (Innovation 3: Protocol-Aware Observability).
//!
//! Provides structured tracing spans with IRC-specific context, enabling
//! correlation of logs across command processing, channel operations, and
//! message routing.
//!
//! ## Key Features
//!
//! - `IrcTraceContext`: Captures IRC-specific attributes (command, channel, msgid, source_nick)
//! - `irc_span!`: Macro to create tracing spans with IRC context
//! - Integration with the metrics module for unified observability

// Allow unused items - these are public APIs for extensibility
#![allow(dead_code)]

use std::time::Instant;
use tracing::{Level, Span, span};

/// IRC-specific trace context for structured logging.
///
/// Captures the key attributes of an IRC operation for correlation
/// across distributed traces and log aggregation.
#[derive(Debug, Clone, Default)]
pub struct IrcTraceContext {
    /// The IRC command being processed (e.g., "PRIVMSG", "JOIN").
    pub command: Option<String>,
    /// Target channel, if applicable.
    pub channel: Option<String>,
    /// IRCv3 msgid tag for message correlation.
    pub msgid: Option<String>,
    /// Source nickname.
    pub source_nick: Option<String>,
    /// Target nickname (for PRIVMSG/NOTICE to users).
    pub target_nick: Option<String>,
    /// User's unique ID.
    pub uid: Option<String>,
    /// Client IP address (cloaked or real depending on context).
    pub client_ip: Option<String>,
    /// Whether this is a TLS connection.
    pub is_tls: bool,
    /// Account name if authenticated.
    pub account: Option<String>,
}

impl IrcTraceContext {
    /// Create a new empty trace context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the command being processed.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set the target channel.
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }

    /// Set the IRCv3 msgid for correlation.
    pub fn with_msgid(mut self, msgid: impl Into<String>) -> Self {
        self.msgid = Some(msgid.into());
        self
    }

    /// Set the source nickname.
    pub fn with_source_nick(mut self, nick: impl Into<String>) -> Self {
        self.source_nick = Some(nick.into());
        self
    }

    /// Set the target nickname.
    pub fn with_target_nick(mut self, nick: impl Into<String>) -> Self {
        self.target_nick = Some(nick.into());
        self
    }

    /// Set the user's UID.
    pub fn with_uid(mut self, uid: impl Into<String>) -> Self {
        self.uid = Some(uid.into());
        self
    }

    /// Set the client IP.
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Set TLS status.
    pub fn with_tls(mut self, is_tls: bool) -> Self {
        self.is_tls = is_tls;
        self
    }

    /// Set the account name.
    pub fn with_account(mut self, account: impl Into<String>) -> Self {
        self.account = Some(account.into());
        self
    }

    /// Create a tracing span from this context.
    ///
    /// The span includes all set attributes as structured fields.
    pub fn into_span(self) -> Span {
        let command = self.command.as_deref().unwrap_or("unknown");

        // Create span with dynamic fields based on what's set
        span!(
            Level::INFO,
            "irc.command",
            command = command,
            channel = self.channel.as_deref(),
            msgid = self.msgid.as_deref(),
            source_nick = self.source_nick.as_deref(),
            target_nick = self.target_nick.as_deref(),
            uid = self.uid.as_deref(),
            client_ip = self.client_ip.as_deref(),
            is_tls = self.is_tls,
            account = self.account.as_deref(),
        )
    }
}

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

    /// Get elapsed time since timer started.
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Stop the timer and record an error (does not record duration).
    pub fn record_error(self, error: &str) {
        crate::metrics::record_command_error(&self.command, error);
    }
}

impl Drop for CommandTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        crate::metrics::record_command(&self.command, duration);
    }
}

/// Create an IRC-aware tracing span for a command.
///
/// This is a convenience function that combines `IrcTraceContext` creation
/// with span creation.
///
/// # Example
///
/// ```ignore
/// let _span = create_irc_span("PRIVMSG", Some("#channel"), Some("user123"));
/// // ... handle command ...
/// // span is automatically closed when dropped
/// ```
pub fn create_irc_span(
    command: &str,
    channel: Option<&str>,
    source_nick: Option<&str>,
) -> Span {
    let mut ctx = IrcTraceContext::new().with_command(command);

    if let Some(ch) = channel {
        ctx = ctx.with_channel(ch);
    }

    if let Some(nick) = source_nick {
        ctx = ctx.with_source_nick(nick);
    }

    ctx.into_span()
}

/// Create a span for channel operations.
pub fn create_channel_span(channel: &str, operation: &str) -> Span {
    span!(
        Level::DEBUG,
        "irc.channel",
        channel = channel,
        operation = operation,
    )
}

/// Create a span for message routing with fan-out tracking.
pub fn create_message_span(
    channel: &str,
    sender: &str,
    recipients: usize,
) -> Span {
    // Also record fan-out metric
    crate::metrics::record_fanout(recipients);

    span!(
        Level::DEBUG,
        "irc.message",
        channel = channel,
        sender = sender,
        recipients = recipients,
    )
}

/// Extract msgid from message tags if present.
pub fn extract_msgid(msg: &slirc_proto::MessageRef<'_>) -> Option<String> {
    msg.tags_iter()
        .find(|(k, _)| *k == "msgid")
        .map(|(_, v)| v.to_string())
}

/// Extract label from message tags if present (for labeled-response).
pub fn extract_label(msg: &slirc_proto::MessageRef<'_>) -> Option<String> {
    msg.tags_iter()
        .find(|(k, _)| *k == "label")
        .map(|(_, v)| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_builder() {
        let ctx = IrcTraceContext::new()
            .with_command("PRIVMSG")
            .with_channel("#test")
            .with_source_nick("alice")
            .with_target_nick("bob")
            .with_uid("001AAAAAA")
            .with_tls(true);

        assert_eq!(ctx.command.as_deref(), Some("PRIVMSG"));
        assert_eq!(ctx.channel.as_deref(), Some("#test"));
        assert_eq!(ctx.source_nick.as_deref(), Some("alice"));
        assert_eq!(ctx.target_nick.as_deref(), Some("bob"));
        assert_eq!(ctx.uid.as_deref(), Some("001AAAAAA"));
        assert!(ctx.is_tls);
    }

    #[test]
    fn test_command_timer() {
        let timer = CommandTimer::new("TEST");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.elapsed_secs();
        assert!(elapsed >= 0.01);
        // Timer records on drop
        drop(timer);
    }
}
