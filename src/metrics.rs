//! Prometheus metrics collection for slircd-ng.
//!
//! Provides production-ready observability via Prometheus metrics exposed on
//! an HTTP endpoint. Tracks server health, message throughput, security events,
//! and user/channel statistics.

use lazy_static::lazy_static;
use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};

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
}

/// Initialize the Prometheus metrics registry.
///
/// Must be called once at server startup before any metrics are recorded.
pub fn init() {
    REGISTRY
        .register(Box::new(MESSAGES_SENT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(SPAM_BLOCKED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(BANS_TRIGGERED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(XLINES_ENFORCED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(RATE_LIMITED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(REGISTERED_ONLY_BLOCKED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(CONNECTED_USERS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(ACTIVE_CHANNELS.clone()))
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
