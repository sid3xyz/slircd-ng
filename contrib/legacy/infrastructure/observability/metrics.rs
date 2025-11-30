/// Label for S2S link metrics (server name)
#[derive(Debug, Clone, PartialEq, Eq, Hash, EncodeLabelSet)]
pub struct S2SLinkLabel {
    pub server_name: String,
}

/// Performance monitoring and metrics collection for SLIRCd IRC server.
///
/// This module provides comprehensive metrics tracking for connection patterns,
/// message processing, channel operations, and system performance. Metrics are
/// exported in OpenMetrics format compatible with Prometheus and other monitoring systems.
/// Labels are stable for dashboards; new metrics will be additive.
use std::time::Duration;
use vise::{
    Buckets, Counter, EncodeLabelSet, EncodeLabelValue, Family, Gauge, Global, Histogram, Info,
    Metrics, Unit,
};

/// IRC command types for command-specific metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EncodeLabelValue, EncodeLabelSet)]
#[metrics(label = "command")]
pub enum IrcCommand {
    #[metrics(name = "nick")]
    Nick,
    #[metrics(name = "user")]
    User,
    #[metrics(name = "join")]
    Join,
    #[metrics(name = "part")]
    Part,
    #[metrics(name = "privmsg")]
    Privmsg,
    #[metrics(name = "notice")]
    Notice,
    #[metrics(name = "quit")]
    Quit,
    #[metrics(name = "kick")]
    Kick,
    #[metrics(name = "mode")]
    Mode,
    #[metrics(name = "topic")]
    Topic,
    #[metrics(name = "who")]
    Who,
    #[metrics(name = "list")]
    List,
    #[metrics(name = "ping")]
    Ping,
    #[metrics(name = "pong")]
    Pong,
    #[metrics(name = "cap")]
    Cap,
    #[metrics(name = "away")]
    Away,
    #[metrics(name = "version")]
    Version,
    #[metrics(name = "other")]
    Other,
}

impl std::fmt::Display for IrcCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrcCommand::Nick => write!(f, "nick"),
            IrcCommand::User => write!(f, "user"),
            IrcCommand::Join => write!(f, "join"),
            IrcCommand::Part => write!(f, "part"),
            IrcCommand::Privmsg => write!(f, "privmsg"),
            IrcCommand::Notice => write!(f, "notice"),
            IrcCommand::Quit => write!(f, "quit"),
            IrcCommand::Kick => write!(f, "kick"),
            IrcCommand::Mode => write!(f, "mode"),
            IrcCommand::Topic => write!(f, "topic"),
            IrcCommand::Who => write!(f, "who"),
            IrcCommand::List => write!(f, "list"),
            IrcCommand::Ping => write!(f, "ping"),
            IrcCommand::Pong => write!(f, "pong"),
            IrcCommand::Cap => write!(f, "cap"),
            IrcCommand::Away => write!(f, "away"),
            IrcCommand::Version => write!(f, "version"),
            IrcCommand::Other => write!(f, "other"),
        }
    }
}

/// Connection types for connection-specific metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EncodeLabelValue, EncodeLabelSet)]
#[metrics(label = "connection_type")]
pub enum ConnectionType {
    #[metrics(name = "plaintext")]
    Plaintext,
    #[metrics(name = "tls")]
    Tls,
}

/// Channel label for top channels metric (Backend API integration)
#[derive(Debug, Clone, PartialEq, Eq, Hash, EncodeLabelSet)]
pub struct ChannelLabel {
    pub channel: String,
}

impl std::fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionType::Plaintext => write!(f, "plaintext"),
            ConnectionType::Tls => write!(f, "tls"),
        }
    }
}

/// Error types for error-specific metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EncodeLabelValue, EncodeLabelSet)]
#[metrics(label = "error_type")]
pub enum ErrorType {
    #[metrics(name = "protocol")]
    Protocol,
    #[metrics(name = "database")]
    Database,
    #[metrics(name = "network")]
    Network,
    #[metrics(name = "authentication")]
    Authentication,
    #[metrics(name = "rate_limit")]
    RateLimit,
    #[metrics(name = "channel")]
    Channel,
    #[metrics(name = "internal")]
    Internal,
}

impl std::fmt::Display for ErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorType::Protocol => write!(f, "protocol"),
            ErrorType::Database => write!(f, "database"),
            ErrorType::Network => write!(f, "network"),
            ErrorType::Authentication => write!(f, "authentication"),
            ErrorType::RateLimit => write!(f, "rate_limit"),
            ErrorType::Channel => write!(f, "channel"),
            ErrorType::Internal => write!(f, "internal"),
        }
    }
}

/// Disconnection reasons for tracking why clients disconnect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EncodeLabelValue, EncodeLabelSet)]
#[metrics(label = "reason")]
pub enum DisconnectReason {
    #[metrics(name = "quit")]
    Quit,
    #[metrics(name = "timeout")]
    Timeout,
    #[metrics(name = "error")]
    Error,
    #[metrics(name = "kick")]
    Kick,
    #[metrics(name = "rate_limit")]
    RateLimit,
    #[metrics(name = "shutdown")]
    Shutdown,
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisconnectReason::Quit => write!(f, "quit"),
            DisconnectReason::Timeout => write!(f, "timeout"),
            DisconnectReason::Error => write!(f, "error"),
            DisconnectReason::Kick => write!(f, "kick"),
            DisconnectReason::RateLimit => write!(f, "rate_limit"),
            DisconnectReason::Shutdown => write!(f, "shutdown"),
        }
    }
}

/// Reasons for rejecting a connection before it is fully established
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EncodeLabelValue, EncodeLabelSet)]
#[metrics(label = "reason")]
pub enum ConnectionRejectReason {
    /// Rejected due to global connection rate limiter (new connections/min)
    #[metrics(name = "rate_limit")]
    RateLimit,
    /// Rejected due to reaching the global maximum concurrent connections
    #[metrics(name = "max_conn")]
    MaxConn,
    /// Rejected because the per-IP concurrent connection limit was reached
    #[metrics(name = "per_ip_limit")]
    PerIpLimit,
    /// Rejected due to TLS handshake error
    #[metrics(name = "tls_error")]
    TlsError,
    /// Rejected due to DNSBL (DNS blacklist) listing
    #[metrics(name = "dnsbl")]
    Dnsbl,
    /// Rejected due to Z-line/D-line IP ban
    #[metrics(name = "banned")]
    Banned,
}

impl std::fmt::Display for ConnectionRejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionRejectReason::RateLimit => write!(f, "rate_limit"),
            ConnectionRejectReason::MaxConn => write!(f, "max_conn"),
            ConnectionRejectReason::PerIpLimit => write!(f, "per_ip_limit"),
            ConnectionRejectReason::TlsError => write!(f, "tls_error"),
            ConnectionRejectReason::Dnsbl => write!(f, "dnsbl"),
            ConnectionRejectReason::Banned => write!(f, "banned"),
        }
    }
}

/// Comprehensive metrics for SLIRCd IRC server performance monitoring

#[derive(Debug, Clone, Metrics)]
#[metrics(prefix = "slircd")]
pub struct SlircdMetrics {
    // --- S2S LINK METRICS ---
    /// Total S2S autoconnect attempts (RFC 2813 §4.1, competitive: Solanum/Ergo)
    pub s2s_autoconnect_attempts: Counter,

    /// S2S autoconnect successes (per-server)
    pub s2s_autoconnect_successes: Family<S2SLinkLabel, Counter>,

    /// S2S autoconnect failures (per-server)
    pub s2s_autoconnect_failures: Family<S2SLinkLabel, Counter>,

    /// S2S link disconnects (per-server)
    pub s2s_link_disconnects: Family<S2SLinkLabel, Counter>,

    /// S2S autoconnect retries (per-server)
    pub s2s_autoconnect_retries: Family<S2SLinkLabel, Counter>,

    /// SYNAPSE commands sent (REHASH propagation) - EXCEEDS BIG 4
    pub s2s_commands_sent: Counter,

    /// SYNAPSE commands received from linked servers
    pub s2s_commands_received: Counter,

    /// Total config reload attempts
    pub config_reload_attempts_total: Counter,

    /// Total config reload failures
    pub config_reload_failures_total: Counter,

    /// Total successful config reloads
    pub config_reloads_total: Counter,
    /// Total number of client connections established
    pub connections_total: Counter,

    /// Currently active client connections
    pub connections_active: Gauge<u64>,

    /// Client connections by type (TLS/plaintext)
    pub connections_by_type: Family<ConnectionType, Gauge<u64>>,

    /// Connection establishment latency
    #[metrics(buckets = Buckets::LATENCIES, unit = Unit::Seconds)]
    pub connection_setup_duration: Histogram<Duration>,

    /// Client disconnections by reason
    pub disconnections: Family<DisconnectReason, Counter>,

    /// Connection rejections by reason (before fully established)
    pub connection_rejections: Family<ConnectionRejectReason, Counter>,

    /// Total messages processed
    pub messages_total: Counter,

    /// Messages processed by command type
    pub messages_by_command: Family<IrcCommand, Counter>,

    /// Message processing latency
    #[metrics(buckets = Buckets::LATENCIES, unit = Unit::Seconds)]
    pub message_processing_duration: Histogram<Duration>,

    /// Command execution time per command type (MED-3 enhancement)
    #[metrics(buckets = Buckets::LATENCIES, unit = Unit::Seconds)]
    pub command_duration: Family<IrcCommand, Histogram<Duration>>,

    /// Rate limiting events
    pub rate_limit_hits: Counter,

    /// Network bytes sent
    #[metrics(unit = Unit::Bytes)]
    pub bytes_sent: Counter,

    /// Network bytes received
    #[metrics(unit = Unit::Bytes)]
    pub bytes_received: Counter,

    /// Currently active channels
    pub channels_active: Gauge<u64>,

    /// Channel operations (join/part/kick/etc)
    pub channel_operations: Family<IrcCommand, Counter>,

    /// Users per channel distribution
    #[metrics(buckets = Buckets::exponential(1.0..=1000.0, 2.0))]
    pub users_per_channel: Histogram<u64>,

    /// Database operation latency
    #[metrics(buckets = Buckets::LATENCIES, unit = Unit::Seconds)]
    pub database_duration: Histogram<Duration>,

    /// Database operations total
    pub database_operations: Counter,

    /// Errors by type
    pub errors: Family<ErrorType, Counter>,

    /// IRCv3 capability negotiations successful
    pub capability_negotiations: Counter,

    /// Memory usage by the server process
    #[metrics(unit = Unit::Bytes)]
    pub memory_usage: Gauge<u64>,

    /// DashMap access latency (for concurrent state operations)
    #[metrics(buckets = Buckets::exponential(1e-6..=1.0, 10.0), unit = Unit::Seconds)]
    pub state_access_duration: Histogram<Duration>,

    /// Number of authentication attempts
    pub authentication_attempts: Counter,

    /// Number of successful authentications
    pub authentication_successes: Counter,

    /// Message queue backlog size per client
    #[metrics(buckets = Buckets::exponential(1.0..=10000.0, 2.0))]
    pub message_queue_size: Histogram<u64>,

    /// Currently active IRC operators (Backend API integration)
    pub operators_active: Gauge<u64>,

    /// Server uptime in seconds (Backend API integration)
    pub server_uptime_seconds: Gauge<u64>,

    /// Process CPU usage percentage (Backend API integration)
    pub cpu_usage_percent: Gauge<f64>,

    /// Top channels by user count (Backend API integration)
    pub top_channels_by_users: Family<ChannelLabel, Gauge<u64>>,
    // --- END S2S LINK METRICS ---
}

/// Global metrics instance for the slircd server
#[vise::register]
pub static METRICS: Global<SlircdMetrics> = Global::new();

/// Server information metrics
#[derive(Debug, Clone, Metrics)]
#[metrics(prefix = "slircd_info")]
pub struct ServerInfo {
    /// Server version and build information
    pub version: Info<()>,
}

/// Global server information
#[vise::register]
pub static SERVER_INFO: Global<ServerInfo> = Global::new();

/// Initialize server information metrics with version data
pub fn init_server_info() {
    // For Info<()>, we just set the unit value to indicate server is active
    if let Err(e) = SERVER_INFO.version.set(()) {
        tracing::warn!("Failed to set server info metrics: {}", e);
    }
}

/// Helper function to convert command strings to IrcCommand enum
/// RUST ARCHITECT: ✅ Zero-allocation hot path - case-insensitive match without to_uppercase()
#[inline]
pub fn command_to_metric(command: &str) -> IrcCommand {
    // PERFORMANCE: Use eq_ignore_ascii_case to avoid allocation from to_uppercase()
    match command {
        s if s.eq_ignore_ascii_case("NICK") => IrcCommand::Nick,
        s if s.eq_ignore_ascii_case("USER") => IrcCommand::User,
        s if s.eq_ignore_ascii_case("JOIN") => IrcCommand::Join,
        s if s.eq_ignore_ascii_case("PART") => IrcCommand::Part,
        s if s.eq_ignore_ascii_case("PRIVMSG") => IrcCommand::Privmsg,
        s if s.eq_ignore_ascii_case("NOTICE") => IrcCommand::Notice,
        s if s.eq_ignore_ascii_case("QUIT") => IrcCommand::Quit,
        s if s.eq_ignore_ascii_case("KICK") => IrcCommand::Kick,
        s if s.eq_ignore_ascii_case("MODE") => IrcCommand::Mode,
        s if s.eq_ignore_ascii_case("TOPIC") => IrcCommand::Topic,
        s if s.eq_ignore_ascii_case("WHO") => IrcCommand::Who,
        s if s.eq_ignore_ascii_case("LIST") => IrcCommand::List,
        s if s.eq_ignore_ascii_case("PING") => IrcCommand::Ping,
        s if s.eq_ignore_ascii_case("PONG") => IrcCommand::Pong,
        s if s.eq_ignore_ascii_case("CAP") => IrcCommand::Cap,
        s if s.eq_ignore_ascii_case("AWAY") => IrcCommand::Away,
        s if s.eq_ignore_ascii_case("VERSION") => IrcCommand::Version,
        _ => IrcCommand::Other,
    }
}

/// Start background task for periodic memory usage tracking (MED-3 enhancement)
/// Updates `memory_usage` gauge every 30 seconds with process RSS (resident set size)
pub fn start_memory_tracking() {
    tokio::spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;

            if let Some(usage) = memory_stats::memory_stats() {
                // Physical memory (RSS) is the primary metric for process memory consumption
                METRICS.memory_usage.set(usage.physical_mem as u64);
            } else {
                // memory_stats failed (unsupported platform or permission issue)
                tracing::warn!("Failed to retrieve memory stats");
            }
        }
    });
}

/// Start background task for server uptime tracking (Backend API integration)
/// Updates `server_uptime_seconds` gauge every 30 seconds with elapsed time since start
pub fn start_uptime_tracking(start_time: std::time::Instant) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let uptime_secs = start_time.elapsed().as_secs();
            METRICS.server_uptime_seconds.set(uptime_secs);
        }
    });
}

/// Start background task for CPU usage tracking (Backend API integration)
/// Updates `cpu_usage_percent` gauge every 30 seconds with process-specific CPU percentage
pub fn start_cpu_tracking() {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};

    tokio::spawn(async move {
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new().with_cpu()),
        );
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

        // First refresh to initialize CPU tracking
        sys.refresh_processes_specifics(ProcessRefreshKind::new().with_cpu());

        loop {
            interval.tick().await;

            // Refresh process CPU usage
            sys.refresh_processes_specifics(ProcessRefreshKind::new().with_cpu());

            if let Some(process) = sys.process(pid) {
                // cpu_usage() returns percentage (0.0-100.0+)
                let cpu_percent = process.cpu_usage() as f64;
                METRICS.cpu_usage_percent.set(cpu_percent);
            } else {
                tracing::warn!("Failed to retrieve process CPU stats");
            }
        }
    });
}

/// Start background task for top channels tracking (Backend API integration)
/// Updates `top_channels_by_users` every 60 seconds with the top 5 channels by member count
pub fn start_top_channels_tracking(state: std::sync::Arc<crate::core::state::ServerState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

        loop {
            interval.tick().await;

            // Get all channels sorted by member count (descending)
            let channel_stats = state.get_channel_stats();

            // Update metrics for top 5 channels
            for (channel_name, member_count) in channel_stats.iter().take(5) {
                METRICS.top_channels_by_users[&ChannelLabel {
                    channel: channel_name.clone(),
                }]
                    .set(*member_count as u64);
            }
        }
    });
}
