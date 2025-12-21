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
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref MESSAGES_SENT: IntCounter = IntCounter::new(
        "irc_messages_sent_total",
        "Total messages sent"
    ).expect("MESSAGES_SENT metric creation failed");

    /// Total messages blocked by spam detection.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref SPAM_BLOCKED: IntCounter = IntCounter::new(
        "irc_spam_blocked_total",
        "Messages blocked as spam"
    ).expect("SPAM_BLOCKED metric creation failed");

    /// Total ban enforcement events (channel bans blocking JOIN).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref BANS_TRIGGERED: IntCounter = IntCounter::new(
        "irc_bans_triggered_total",
        "Ban enforcement events"
    ).expect("BANS_TRIGGERED metric creation failed");

    /// Total X-line enforcement events (K/G/Z/R/S-lines blocking connections).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref XLINES_ENFORCED: IntCounter = IntCounter::new(
        "irc_xlines_enforced_total",
        "X-line enforcement events"
    ).expect("XLINES_ENFORCED metric creation failed");

    /// Total rate limit hits (flood protection).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref RATE_LIMITED: IntCounter = IntCounter::new(
        "irc_rate_limited_total",
        "Rate limit hits"
    ).expect("RATE_LIMITED metric creation failed");

    /// Total +r (registered-only) enforcement events (JOIN/speak denied).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref REGISTERED_ONLY_BLOCKED: IntCounter = IntCounter::new(
        "irc_registered_only_blocked_total",
        "Registered-only (+r) enforcement events"
    ).expect("REGISTERED_ONLY_BLOCKED metric creation failed");

    // ========================================================================
    // Gauges (can increase/decrease)
    // ========================================================================

    /// Currently connected users.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref CONNECTED_USERS: IntGauge = IntGauge::new(
        "irc_connected_users",
        "Currently connected users"
    ).expect("CONNECTED_USERS metric creation failed");

    /// Active channels (both registered and temporary).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref ACTIVE_CHANNELS: IntGauge = IntGauge::new(
        "irc_active_channels",
        "Active channels"
    ).expect("ACTIVE_CHANNELS metric creation failed");

    // ========================================================================
    // IRC-Specific Metrics (Innovation 3: Protocol-Aware Observability)
    // ========================================================================

    /// Commands processed by type (PRIVMSG, JOIN, PART, etc.).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref COMMAND_COUNTER: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_command_total", "IRC commands processed by type"),
        &["command"]
    ).expect("COMMAND_COUNTER metric creation failed");

    /// Command processing latency by command type.
    /// Buckets optimized for IRC: 50µs to 500ms.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref COMMAND_LATENCY: HistogramVec = HistogramVec::new(
        HistogramOpts::new("irc_command_duration_seconds", "IRC command latency by type")
            .buckets(vec![0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        &["command"]
    ).expect("COMMAND_LATENCY metric creation failed");

    /// Channel member counts (gauge).
    /// Updated on JOIN/PART/KICK/QUIT.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref CHANNEL_MEMBERS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("irc_channel_members", "Members per IRC channel"),
        &["channel"]
    ).expect("CHANNEL_MEMBERS metric creation failed");

    /// Message fan-out histogram: how many recipients per channel message.
    /// Buckets: 1, 5, 10, 25, 50, 100, 250, 500, 1000+.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref MESSAGE_FANOUT: Histogram = Histogram::with_opts(
        HistogramOpts::new("irc_message_fanout", "Recipients per channel message")
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0])
    ).expect("MESSAGE_FANOUT metric creation failed");

    /// Command errors by type and error kind.
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref COMMAND_ERRORS: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_command_errors_total", "IRC command errors by type"),
        &["command", "error"]
    ).expect("COMMAND_ERRORS metric creation failed");

    /// Channel mode changes (counter).
    // SAFETY: Metrics init at startup via lazy_static, panic acceptable if prometheus fails
    pub static ref CHANNEL_MODE_CHANGES: IntCounterVec = IntCounterVec::new(
        Opts::new("irc_channel_mode_changes_total", "Channel mode changes"),
        &["mode"]
    ).expect("CHANNEL_MODE_CHANGES metric creation failed");

    // ========================================================================
    // Distributed System Metrics (Innovation 3, Phase 2)
    // ========================================================================

    /// Distributed messages routed between servers.
    /// Labels: source_sid, target_sid, status (success/failure)
    pub static ref DISTRIBUTED_MESSAGES_ROUTED: IntCounterVec = IntCounterVec::new(
        Opts::new("slircd_distributed_messages_routed_total", "Messages routed between servers"),
        &["source_sid", "target_sid", "status"]
    ).expect("DISTRIBUTED_MESSAGES_ROUTED metric creation failed");

    /// Distributed collisions (nick/channel) resolved.
    /// Labels: type (nick/channel), resolution (kill/merge)
    pub static ref DISTRIBUTED_COLLISIONS_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new("slircd_distributed_collisions_total", "Distributed collisions resolved"),
        &["type", "resolution"]
    ).expect("DISTRIBUTED_COLLISIONS_TOTAL metric creation failed");

    /// Distributed sync latency (processing time for DELTA messages).
    /// Labels: peer_sid
    pub static ref DISTRIBUTED_SYNC_LATENCY: HistogramVec = HistogramVec::new(
        HistogramOpts::new("slircd_distributed_sync_latency_seconds", "Processing time for sync messages")
            .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        &["peer_sid"]
    ).expect("DISTRIBUTED_SYNC_LATENCY metric creation failed");

    /// Connected distributed peers.
    pub static ref DISTRIBUTED_PEERS_CONNECTED: IntGauge = IntGauge::new(
        "slircd_distributed_peers_connected",
        "Number of connected distributed peers"
    ).expect("DISTRIBUTED_PEERS_CONNECTED metric creation failed");
}

/// Initialize the Prometheus metrics registry.
///
/// Must be called once at server startup before any metrics are recorded.
pub fn init() {
    // Legacy counters
    if let Err(e) = REGISTRY.register(Box::new(MESSAGES_SENT.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_messages_sent_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(SPAM_BLOCKED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_spam_blocked_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(BANS_TRIGGERED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_bans_triggered_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(XLINES_ENFORCED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_xlines_enforced_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(RATE_LIMITED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_rate_limited_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(REGISTERED_ONLY_BLOCKED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_registered_only_blocked_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(CONNECTED_USERS.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_connected_users");
    }
    if let Err(e) = REGISTRY.register(Box::new(ACTIVE_CHANNELS.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_active_channels");
    }

    // IRC-specific metrics (Innovation 3)
    if let Err(e) = REGISTRY.register(Box::new(COMMAND_COUNTER.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_command_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(COMMAND_LATENCY.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_command_duration_seconds");
    }
    if let Err(e) = REGISTRY.register(Box::new(CHANNEL_MEMBERS.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_channel_members");
    }
    if let Err(e) = REGISTRY.register(Box::new(MESSAGE_FANOUT.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_message_fanout");
    }
    if let Err(e) = REGISTRY.register(Box::new(COMMAND_ERRORS.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_command_errors_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(CHANNEL_MODE_CHANGES.clone())) {
        tracing::warn!(error = %e, "Failed to register metric irc_channel_mode_changes_total");
    }

    // Distributed metrics (Innovation 3, Phase 2)
    if let Err(e) = REGISTRY.register(Box::new(DISTRIBUTED_MESSAGES_ROUTED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric slircd_distributed_messages_routed_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(DISTRIBUTED_COLLISIONS_TOTAL.clone())) {
        tracing::warn!(error = %e, "Failed to register metric slircd_distributed_collisions_total");
    }
    if let Err(e) = REGISTRY.register(Box::new(DISTRIBUTED_SYNC_LATENCY.clone())) {
        tracing::warn!(error = %e, "Failed to register metric slircd_distributed_sync_latency_seconds");
    }
    if let Err(e) = REGISTRY.register(Box::new(DISTRIBUTED_PEERS_CONNECTED.clone())) {
        tracing::warn!(error = %e, "Failed to register metric slircd_distributed_peers_connected");
    }
}

/// Gather all metrics and encode them in Prometheus text format.
///
/// Returns a string suitable for HTTP response on `/metrics` endpoint.
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = vec![];
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!(error = %e, "Failed to encode Prometheus metrics");
        return String::new();
    }
    match String::from_utf8(buffer) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Prometheus metrics were not valid UTF-8");
            String::new()
        }
    }
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
