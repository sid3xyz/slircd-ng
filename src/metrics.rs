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

use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::sync::OnceLock;

/// Global Prometheus registry for all metrics.
pub static REGISTRY: OnceLock<Registry> = OnceLock::new();

pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::new)
}

// ========================================================================
// Counters (monotonic increasing)
// ========================================================================

/// Total IRC messages successfully sent to clients.
pub static MESSAGES_SENT: OnceLock<IntCounter> = OnceLock::new();

/// Total messages blocked by spam detection.
pub static SPAM_BLOCKED: OnceLock<IntCounter> = OnceLock::new();

/// Total ban enforcement events (channel bans blocking JOIN).
pub static BANS_TRIGGERED: OnceLock<IntCounter> = OnceLock::new();

/// Total X-line enforcement events (K/G/Z/R/S-lines blocking connections).
pub static XLINES_ENFORCED: OnceLock<IntCounter> = OnceLock::new();

/// Total rate limit hits (flood protection).
pub static RATE_LIMITED: OnceLock<IntCounter> = OnceLock::new();

/// Total +r (registered-only) enforcement events (JOIN/speak denied).
pub static REGISTERED_ONLY_BLOCKED: OnceLock<IntCounter> = OnceLock::new();

// ========================================================================
// Gauges (can increase/decrease)
// ========================================================================

/// Currently connected users.
pub static CONNECTED_USERS: OnceLock<IntGauge> = OnceLock::new();

/// Active channels (both registered and temporary).
pub static ACTIVE_CHANNELS: OnceLock<IntGauge> = OnceLock::new();

// ========================================================================
// IRC-Specific Metrics (Innovation 3: Protocol-Aware Observability)
// ========================================================================

/// Commands processed by type (PRIVMSG, JOIN, PART, etc.).
pub static COMMAND_COUNTER: OnceLock<IntCounterVec> = OnceLock::new();

/// Command processing latency by command type.
pub static COMMAND_LATENCY: OnceLock<HistogramVec> = OnceLock::new();

/// Channel member counts (gauge).
pub static CHANNEL_MEMBERS: OnceLock<IntGaugeVec> = OnceLock::new();

/// Message fan-out histogram: how many recipients per channel message.
pub static MESSAGE_FANOUT: OnceLock<Histogram> = OnceLock::new();

/// Command errors by type and error kind.
pub static COMMAND_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();

pub static CHANNEL_MODE_CHANGES: OnceLock<IntCounterVec> = OnceLock::new();

/// Channel messages dropped due to SendQ/backpressure.
pub static CHANNEL_MESSAGES_DROPPED: OnceLock<IntCounter> = OnceLock::new();

// ========================================================================
// Distributed System Metrics (Innovation 3, Phase 2)
// ========================================================================

/// Distributed messages routed between servers.
pub static DISTRIBUTED_MESSAGES_ROUTED: OnceLock<IntCounterVec> = OnceLock::new();

/// Distributed collisions (nick/channel) resolved.
pub static DISTRIBUTED_COLLISIONS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();

/// Distributed sync latency (processing time for DELTA messages).
pub static DISTRIBUTED_SYNC_LATENCY: OnceLock<HistogramVec> = OnceLock::new();

/// Connected distributed peers.
pub static DISTRIBUTED_PEERS_CONNECTED: OnceLock<IntGauge> = OnceLock::new();

/// S2S bytes sent.
pub static S2S_BYTES_SENT: OnceLock<IntCounterVec> = OnceLock::new();

/// S2S bytes received.
pub static S2S_BYTES_RECEIVED: OnceLock<IntCounterVec> = OnceLock::new();

/// S2S commands processed.
pub static S2S_COMMANDS: OnceLock<IntCounterVec> = OnceLock::new();

/// S2S rate limit events.
pub static S2S_RATE_LIMITED: OnceLock<IntCounterVec> = OnceLock::new();

/// Initialize the Prometheus metrics registry.
///
/// Must be called once at server startup before any metrics are recorded.
pub fn init() {
    let r = registry();

    // Helper macro to register metric
    macro_rules! register {
        ($metric:ident, $init:expr) => {
            let m = $init.expect(concat!(stringify!($metric), " creation failed"));
            if let Err(e) = r.register(Box::new(m.clone())) {
                tracing::warn!(error = %e, concat!("Failed to register metric ", stringify!($metric)));
            }
            let _ = $metric.set(m);
        };
    }

    register!(MESSAGES_SENT, IntCounter::new("irc_messages_sent_total", "Total messages sent"));
    register!(SPAM_BLOCKED, IntCounter::new("irc_spam_blocked_total", "Messages blocked as spam"));
    register!(BANS_TRIGGERED, IntCounter::new("irc_bans_triggered_total", "Ban enforcement events"));
    register!(XLINES_ENFORCED, IntCounter::new("irc_xlines_enforced_total", "X-line enforcement events"));
    register!(RATE_LIMITED, IntCounter::new("irc_rate_limited_total", "Rate limit hits"));
    register!(REGISTERED_ONLY_BLOCKED, IntCounter::new("irc_registered_only_blocked_total", "Registered-only (+r) enforcement events"));
    register!(CONNECTED_USERS, IntGauge::new("irc_connected_users", "Currently connected users"));
    register!(ACTIVE_CHANNELS, IntGauge::new("irc_active_channels", "Active channels"));

    register!(COMMAND_COUNTER, IntCounterVec::new(Opts::new("irc_command_total", "IRC commands processed by type"), &["command"]));
    register!(COMMAND_LATENCY, HistogramVec::new(
        HistogramOpts::new("irc_command_duration_seconds", "IRC command latency by type")
            .buckets(vec![0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        &["command"]));
    register!(CHANNEL_MEMBERS, IntGaugeVec::new(Opts::new("irc_channel_members", "Members per IRC channel"), &["channel"]));
    register!(MESSAGE_FANOUT, Histogram::with_opts(
        HistogramOpts::new("irc_message_fanout", "Recipients per channel message")
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0])));
    register!(COMMAND_ERRORS, IntCounterVec::new(Opts::new("irc_command_errors_total", "IRC command errors by type"), &["command", "error"]));
    register!(CHANNEL_MODE_CHANGES, IntCounterVec::new(Opts::new("irc_channel_mode_changes_total", "Channel mode changes"), &["mode"]));
    register!(CHANNEL_MESSAGES_DROPPED, IntCounter::new("irc_channel_messages_dropped_total", "Channel messages dropped due to backpressure"));

    register!(DISTRIBUTED_MESSAGES_ROUTED, IntCounterVec::new(Opts::new("slircd_distributed_messages_routed_total", "Messages routed between servers"), &["source_sid", "target_sid", "status"]));
    register!(DISTRIBUTED_COLLISIONS_TOTAL, IntCounterVec::new(Opts::new("slircd_distributed_collisions_total", "Distributed collisions resolved"), &["type", "resolution"]));
    register!(DISTRIBUTED_SYNC_LATENCY, HistogramVec::new(HistogramOpts::new("slircd_distributed_sync_latency_seconds", "Processing time for sync messages").buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]), &["peer_sid"]));
    register!(DISTRIBUTED_PEERS_CONNECTED, IntGauge::new("slircd_distributed_peers_connected", "Number of connected distributed peers"));
    register!(S2S_BYTES_SENT, IntCounterVec::new(Opts::new("slircd_s2s_bytes_sent_total", "Bytes sent to peer servers"), &["peer_sid"]));
    register!(S2S_BYTES_RECEIVED, IntCounterVec::new(Opts::new("slircd_s2s_bytes_received_total", "Bytes received from peer servers"), &["peer_sid"]));
    register!(S2S_COMMANDS, IntCounterVec::new(Opts::new("slircd_s2s_commands_total", "S2S commands processed"), &["peer_sid", "command"]));
    register!(S2S_RATE_LIMITED, IntCounterVec::new(Opts::new("slircd_s2s_rate_limited_total", "S2S rate limit events"), &["peer_sid", "result"]));
}

/// Gather all metrics and encode them in Prometheus text format.
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = registry().gather();
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



fn get_counter_vec(metric: &OnceLock<IntCounterVec>) -> Option<&IntCounterVec> {
    metric.get()
}



fn get_gauge_vec(metric: &OnceLock<IntGaugeVec>) -> Option<&IntGaugeVec> {
    metric.get()
}

fn get_histogram(metric: &OnceLock<Histogram>) -> Option<&Histogram> {
    metric.get()
}

fn get_histogram_vec(metric: &OnceLock<HistogramVec>) -> Option<&HistogramVec> {
    metric.get()
}

/// Record a command execution with latency.
#[inline]
pub fn record_command(command: &str, duration_secs: f64) {
    if let Some(c) = get_counter_vec(&COMMAND_COUNTER) {
        c.with_label_values(&[command]).inc();
    }
    if let Some(h) = get_histogram_vec(&COMMAND_LATENCY) {
        h.with_label_values(&[command]).observe(duration_secs);
    }
}

/// Record a command error.
#[inline]
pub fn record_command_error(command: &str, error: &str) {
    if let Some(c) = get_counter_vec(&COMMAND_ERRORS) {
        c.with_label_values(&[command, error]).inc();
    }
}

/// Update channel member count gauge.
#[inline]
pub fn set_channel_members(channel: &str, count: i64) {
    if let Some(g) = get_gauge_vec(&CHANNEL_MEMBERS) {
        g.with_label_values(&[channel]).set(count);
    }
}

/// Remove a channel from the members gauge (when channel is destroyed).
#[inline]
pub fn remove_channel_metrics(channel: &str) {
    if let Some(g) = get_gauge_vec(&CHANNEL_MEMBERS) {
        g.with_label_values(&[channel]).set(0);
    }
}

/// Record message fan-out (how many recipients received a channel message).
#[inline]
pub fn record_fanout(recipients: usize) {
    if let Some(h) = get_histogram(&MESSAGE_FANOUT) {
        h.observe(recipients as f64);
    }
}

/// Record a channel mode change.
#[inline]
pub fn record_mode_change(mode: char) {
    if let Some(c) = get_counter_vec(&CHANNEL_MODE_CHANGES) {
        c.with_label_values(&[&mode.to_string()]).inc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_lifecycle() {
        // Init (safe to call multiple times in tests via OnceLock, though technically only runs once)
        init();

        // accessors should work
        record_command("TEST", 0.001);
        
        let output = gather_metrics();
        assert!(output.contains("irc_command_total"));
    }
}
