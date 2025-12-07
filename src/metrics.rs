//! Prometheus metrics collection for slircd-ng.
//!
//! Provides production-ready observability via Prometheus metrics exposed on
//! an HTTP endpoint. Tracks server health, message throughput, security events,
//! and user/channel statistics.
//!
//! ## IRC-Specific Metrics (Innovation 3: Protocol-Aware Observability)
//!
//! - `irc_command_total{command}` - Commands processed by type
//! - `irc_command_duration_seconds{command}` - Command latency histogram
//! - `irc_channel_members{channel}` - Members per channel (gauge)
//! - `irc_message_fanout` - Recipients per channel message (histogram)

use lazy_static::lazy_static;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};

lazy_static! {
    /// Global Prometheus registry for all metrics.
    pub static ref REGISTRY: Registry = Registry::new();

    // ========================================================================
    // Counters (monotonic increasing)
    // ========================================================================

    /// Total IRC messages successfully sent to clients.
    pub static ref MESSAGES_SENT: IntCounter = IntCounter::new(
        "irc_messages_sent_total",
        "Total messages sent"
    ).unwrap();

    /// Total messages blocked by spam detection.
    pub static ref SPAM_BLOCKED: IntCounter = IntCounter::new(
        "irc_spam_blocked_total",
        "Messages blocked as spam"
    ).unwrap();

    /// Total ban enforcement events (channel bans blocking JOIN).
    pub static ref BANS_TRIGGERED: IntCounter = IntCounter::new(
        "irc_bans_triggered_total",
        "Ban enforcement events"
    ).unwrap();

    /// Total X-line enforcement events (K/G/Z/R/S-lines blocking connections).
    pub static ref XLINES_ENFORCED: IntCounter = IntCounter::new(
        "irc_xlines_enforced_total",
        "X-line enforcement events"
    ).unwrap();

    /// Total rate limit hits (flood protection).
    pub static ref RATE_LIMITED: IntCounter = IntCounter::new(
        "irc_rate_limited_total",
        "Rate limit hits"
    ).unwrap();

    /// Total +r (registered-only) enforcement events (JOIN/speak denied).
    pub static ref REGISTERED_ONLY_BLOCKED: IntCounter = IntCounter::new(
        "irc_registered_only_blocked_total",
        "Registered-only (+r) enforcement events"
    ).unwrap();

    // ========================================================================
    // Gauges (can increase/decrease)
    // ========================================================================

    /// Currently connected users.
    pub static ref CONNECTED_USERS: IntGauge = IntGauge::new(
        "irc_connected_users",
        "Currently connected users"
    ).unwrap();

    /// Active channels (both registered and temporary).
    pub static ref ACTIVE_CHANNELS: IntGauge = IntGauge::new(
        "irc_active_channels",
        "Active channels"
    ).unwrap();

    // ========================================================================
    // IRC-Specific Metrics (Innovation 3: Protocol-Aware Observability)
    // ========================================================================

    /// Commands processed by type (PRIVMSG, JOIN, PART, etc.).
    pub static ref COMMAND_COUNTER: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_command_total", "IRC commands processed by type"),
        &["command"]
    ).unwrap();

    /// Command processing latency by command type.
    /// Buckets optimized for IRC: 50µs to 500ms.
    pub static ref COMMAND_LATENCY: HistogramVec = HistogramVec::new(
        HistogramOpts::new("irc_command_duration_seconds", "IRC command latency by type")
            .buckets(vec![0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        &["command"]
    ).unwrap();

    /// Channel member counts (gauge).
    /// Updated on JOIN/PART/KICK/QUIT.
    pub static ref CHANNEL_MEMBERS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("irc_channel_members", "Members per IRC channel"),
        &["channel"]
    ).unwrap();

    /// Message fan-out histogram: how many recipients per channel message.
    /// Buckets: 1, 5, 10, 25, 50, 100, 250, 500, 1000+.
    pub static ref MESSAGE_FANOUT: Histogram = Histogram::with_opts(
        HistogramOpts::new("irc_message_fanout", "Recipients per channel message")
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0])
    ).unwrap();

    /// Command errors by type and error kind.
    pub static ref COMMAND_ERRORS: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_command_errors_total", "IRC command errors by type"),
        &["command", "error"]
    ).unwrap();

    /// Channel mode changes (counter).
    pub static ref CHANNEL_MODE_CHANGES: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_channel_mode_changes_total", "Channel mode changes"),
        &["mode"]
    ).unwrap();
}

/// Initialize the Prometheus metrics registry.
///
/// Must be called once at server startup before any metrics are recorded.
pub fn init() {
    // Legacy counters
    REGISTRY.register(Box::new(MESSAGES_SENT.clone())).unwrap();
    REGISTRY.register(Box::new(SPAM_BLOCKED.clone())).unwrap();
    REGISTRY.register(Box::new(BANS_TRIGGERED.clone())).unwrap();
    REGISTRY
        .register(Box::new(XLINES_ENFORCED.clone()))
        .unwrap();
    REGISTRY.register(Box::new(RATE_LIMITED.clone())).unwrap();
    REGISTRY
        .register(Box::new(REGISTERED_ONLY_BLOCKED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(CONNECTED_USERS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(ACTIVE_CHANNELS.clone()))
        .unwrap();

    // IRC-specific metrics (Innovation 3)
    REGISTRY
        .register(Box::new(COMMAND_COUNTER.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMMAND_LATENCY.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(CHANNEL_MEMBERS.clone()))
        .unwrap();
    REGISTRY.register(Box::new(MESSAGE_FANOUT.clone())).unwrap();
    REGISTRY.register(Box::new(COMMAND_ERRORS.clone())).unwrap();
    REGISTRY
        .register(Box::new(CHANNEL_MODE_CHANGES.clone()))
        .unwrap();
}

/// Gather all metrics and encode them in Prometheus text format.
///
/// Returns a string suitable for HTTP response on `/metrics` endpoint.
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

// ============================================================================
// Helper functions for IRC-specific metric updates
// ============================================================================

/// Record a command execution with latency.
#[inline]
pub fn record_command(command: &str, duration_secs: f64) {
    COMMAND_COUNTER.with_label_values(&[command]).inc();
    COMMAND_LATENCY
        .with_label_values(&[command])
        .observe(duration_secs);
}

/// Record a command error.
#[inline]
pub fn record_command_error(command: &str, error: &str) {
    COMMAND_ERRORS.with_label_values(&[command, error]).inc();
}

/// Update channel member count gauge.
#[inline]
pub fn set_channel_members(channel: &str, count: i64) {
    CHANNEL_MEMBERS.with_label_values(&[channel]).set(count);
}

/// Remove a channel from the members gauge (when channel is destroyed).
#[inline]
pub fn remove_channel_metrics(channel: &str) {
    // Reset to 0 rather than removing (Prometheus doesn't support removal easily)
    CHANNEL_MEMBERS.with_label_values(&[channel]).set(0);
}

/// Record message fan-out (how many recipients received a channel message).
#[inline]
pub fn record_fanout(recipients: usize) {
    MESSAGE_FANOUT.observe(recipients as f64);
}

/// Record a channel mode change.
#[inline]
pub fn record_mode_change(mode: char) {
    CHANNEL_MODE_CHANGES
        .with_label_values(&[&mode.to_string()])
        .inc();
}
