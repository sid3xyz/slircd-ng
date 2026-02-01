//! Prometheus metrics collection using the `metrics` facade.

//!
//! Provides production-ready observability via Prometheus metrics exposed on
//! an HTTP endpoint. Tracks server health, message throughput, security events,
//! and user/channel statistics.

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialize the Prometheus metrics registry.
///
/// Must be called once at server startup before any metrics are recorded.
pub fn init() {
    let builder = PrometheusBuilder::new();
    let handle = match builder.install_recorder() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to install Prometheus recorder: {}", e);
            return;
        }
    };
    let _ = PROMETHEUS_HANDLE.set(handle);

    // Register descriptions
    describe_counter!("irc_messages_sent_total", "Total messages sent");
    describe_counter!("irc_spam_blocked_total", "Messages blocked as spam");
    describe_counter!("irc_bans_triggered_total", "Ban enforcement events");
    describe_counter!("irc_xlines_enforced_total", "X-line enforcement events");
    describe_counter!("irc_rate_limited_total", "Rate limit hits");
    describe_counter!(
        "irc_registered_only_blocked_total",
        "Registered-only (+r) enforcement events"
    );
    describe_gauge!("irc_connected_users", "Currently connected users");
    describe_gauge!("irc_active_channels", "Active channels");

    describe_counter!("irc_command_total", "IRC commands processed by type");
    describe_histogram!(
        "irc_command_duration_seconds",
        "IRC command latency by type"
    );
    describe_gauge!("irc_channel_members", "Members per IRC channel");
    describe_histogram!("irc_message_fanout", "Recipients per channel message");
    describe_counter!("irc_command_errors_total", "IRC command errors by type");
    describe_counter!("irc_channel_mode_changes_total", "Channel mode changes");
    describe_counter!(
        "irc_channel_messages_dropped_total",
        "Channel messages dropped due to backpressure"
    );

    // Distributed System Metrics
    describe_counter!(
        "slircd_distributed_messages_routed_total",
        "Messages routed between servers"
    );
    describe_counter!(
        "slircd_distributed_collisions_total",
        "Distributed collisions resolved"
    );
    describe_histogram!(
        "slircd_distributed_sync_latency_seconds",
        "Processing time for sync messages"
    );
    describe_gauge!(
        "slircd_distributed_peers_connected",
        "Number of connected distributed peers"
    );
    describe_counter!("slircd_s2s_bytes_sent_total", "Bytes sent to peer servers");
    describe_counter!(
        "slircd_s2s_bytes_received_total",
        "Bytes received from peer servers"
    );
    describe_counter!("slircd_s2s_commands_total", "S2S commands processed");
    describe_counter!("slircd_s2s_rate_limited_total", "S2S rate limit events");
}

/// Gather all metrics and encode them in Prometheus text format.
pub fn gather_metrics() -> String {
    PROMETHEUS_HANDLE
        .get()
        .map(|h| h.render())
        .unwrap_or_default()
}

// ========================================================================
// Helper functions (Modernized API)
// ========================================================================

pub fn inc_messages_sent() {
    counter!("irc_messages_sent_total").increment(1);
}

pub fn inc_rate_limited() {
    counter!("irc_rate_limited_total").increment(1);
}

pub fn inc_connected_users() {
    gauge!("irc_connected_users").increment(1.0);
}

pub fn dec_connected_users() {
    gauge!("irc_connected_users").decrement(1.0);
}

pub fn inc_active_channels() {
    gauge!("irc_active_channels").increment(1.0);
}

pub fn dec_active_channels() {
    gauge!("irc_active_channels").decrement(1.0);
}

/// Record a command execution with latency.
#[inline]
pub fn record_command(command: &str, duration_secs: f64) {
    counter!("irc_command_total", "command" => command.to_string()).increment(1);
    histogram!("irc_command_duration_seconds", "command" => command.to_string())
        .record(duration_secs);
}

/// Record a command error.
#[inline]
pub fn record_command_error(command: &str, error: &str) {
    counter!("irc_command_errors_total", "command" => command.to_string(), "error" => error.to_string()).increment(1);
}

/// Update channel member count gauge.
#[inline]
pub fn set_channel_members(channel: &str, count: i64) {
    gauge!("irc_channel_members", "channel" => channel.to_string()).set(count as f64);
}

/// Remove a channel from the members gauge (when channel is destroyed).
#[inline]
pub fn remove_channel_metrics(channel: &str) {
    // There is no "remove" in metrics crate for gauges directly via macro without handle,
    // but setting to 0 is a reasonable fallback for now, or we just stop reporting it.
    // Ideally we would delete the metric, but for now filtering 0s in PromQL is common.
    gauge!("irc_channel_members", "channel" => channel.to_string()).set(0.0);
}

/// Record message fan-out (how many recipients received a channel message).
#[inline]
pub fn record_fanout(recipients: usize) {
    histogram!("irc_message_fanout").record(recipients as f64);
}

/// Record a channel mode change.
#[inline]
pub fn record_mode_change(mode: char) {
    counter!("irc_channel_mode_changes_total", "mode" => mode.to_string()).increment(1);
}

pub fn inc_channel_messages_dropped() {
    counter!("irc_channel_messages_dropped_total").increment(1);
}

// S2S Metrics Helpers

pub fn inc_distributed_messages_routed(source_sid: &str, target_sid: &str, status: &str) {
    counter!(
        "slircd_distributed_messages_routed_total",
        "source_sid" => source_sid.to_string(),
        "target_sid" => target_sid.to_string(),
        "status" => status.to_string()
    )
    .increment(1);
}

pub fn inc_distributed_collisions(kind: &str, resolution: &str) {
    counter!(
        "slircd_distributed_collisions_total",
        "type" => kind.to_string(),
        "resolution" => resolution.to_string()
    )
    .increment(1);
}

pub fn inc_s2s_bytes_sent(peer_sid: &str, bytes: u64) {
    counter!("slircd_s2s_bytes_sent_total", "peer_sid" => peer_sid.to_string()).increment(bytes);
}

pub fn inc_s2s_bytes_received(peer_sid: &str, bytes: u64) {
    counter!("slircd_s2s_bytes_received_total", "peer_sid" => peer_sid.to_string())
        .increment(bytes);
}

pub fn inc_s2s_commands(peer_sid: &str, command: &str) {
    counter!(
        "slircd_s2s_commands_total",
        "peer_sid" => peer_sid.to_string(),
        "command" => command.to_string()
    )
    .increment(1);
}

pub fn inc_s2s_rate_limited(peer_sid: &str, result: &str) {
    counter!(
        "slircd_s2s_rate_limited_total",
        "peer_sid" => peer_sid.to_string(),
        "result" => result.to_string()
    )
    .increment(1);
}
