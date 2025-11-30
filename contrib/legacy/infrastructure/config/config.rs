//! TOML config + env variable fallbacks. Strong typing, validation at startup.
//! RUST ARCHITECT: ✅ 12-factor app design with hot-reload capability

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tracing::{info, warn};

/// IRC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerSection,
    pub listeners: ListenersSection,
    pub database: DatabaseSection,
    pub tls: Option<TlsSection>,
    pub limits: LimitsSection,
    pub logging: LoggingSection,
    pub security: SecuritySection,
    pub webirc: Option<WebircSection>,
    #[serde(default)]
    pub hot_reload: HotReloadSection,
    #[serde(default)]
    pub cloaking: CloakingSection,
    #[serde(default)]
    pub history: HistorySection,
    #[serde(default)]
    pub services: ServicesSection,
    #[serde(default)]
    pub linking: LinkingSection,
    #[serde(default)]
    pub metrics: MetricsSection,
    #[serde(default)]
    pub webadmin: WebAdminSection,
    #[serde(default)]
    pub optional_features: OptionalFeaturesSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSection {
    /// Server name (appears in version replies, etc.)
    pub name: String,

    /// Message of the day file path
    pub motd: Option<String>,

    /// Server administrator info
    pub admin: AdminInfo,

    /// Server-to-server link password (for incoming server connections)
    pub server_password: Option<String>,

    /// Delay in seconds before shutdown to allow users to prepare (default: 10)
    /// Server will send global NOTICE announcing shutdown, wait this many seconds,
    /// then send QUIT messages and terminate
    #[serde(default = "default_shutdown_delay_seconds")]
    pub shutdown_delay_seconds: u64,

    /// UTF-8 strict validation (IRCv3 utf8only)
    /// true = Reject invalid UTF-8 (modern clients only, IRCv3 compliance)
    /// false = Permissive mode (accept all encodings, maximum compatibility)
    /// Default: false (backwards compatible)
    /// Competitive analysis: Matches UnrealIRCd/Ergo opt-in enforcement
    #[serde(default)]
    pub enforce_utf8: bool,

    /// WHOWAS history retention period in days (default: 7)
    /// RFC2812 §3.6.3: Server-defined retention policy for departed users
    /// Automatic cleanup runs daily, removing records older than this threshold
    /// Competitive: UnrealIRCd default 7 days, Ergo 7 days, InspIRCd configurable
    #[serde(default = "default_whowas_retention_days")]
    pub whowas_retention_days: Option<u32>,
}

fn default_whowas_retention_days() -> Option<u32> {
    Some(7)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminInfo {
    /// Administrator location (line 1)
    pub location1: String,

    /// Administrator location (line 2)
    pub location2: String,

    /// Administrator email
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenersSection {
    /// Plain text IRC listener
    pub plaintext: ListenerConfig,

    /// TLS/SSL IRC listener (if TLS is configured)
    pub tls: Option<ListenerConfig>,

    /// WebSocket IRC listener (RFC 6455)
    #[serde(default)]
    pub websocket: Option<ListenerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenerConfig {
    /// Bind address and port
    pub bind: String,

    /// Whether this listener is enabled
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSection {
    /// Database file path
    pub path: String,

    /// Connection pool size
    pub pool_size: usize,

    /// Busy timeout in milliseconds
    pub busy_timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSection {
    /// Path to TLS certificate file
    pub cert_path: String,

    /// Path to TLS private key file
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsSection {
    /// Maximum total connections
    pub max_connections: Option<usize>,

    /// Maximum connections per IP address
    pub max_connections_per_ip: Option<usize>,

    /// Connection rate limit (connections per minute)
    pub connection_rate_per_minute: u32,

    /// Message rate limit per client (messages per second)
    pub message_rate_per_second: f64,

    /// Connection rate per IP (connections per 10 seconds, LOW-2 COMPLETE)
    #[serde(default = "default_connection_rate_per_ip")]
    pub connection_rate_per_ip: u32,

    /// Join rate per client (joins per 10 seconds, LOW-2 COMPLETE)
    #[serde(default = "default_join_rate_per_client")]
    pub join_rate_per_client: u32,

    /// Interval between server-initiated PING keepalives in seconds (RFC2812 §3.7.2)
    #[serde(default = "default_ping_interval_seconds")]
    pub ping_interval_seconds: u64,

    /// Grace period after sending PING before declaring the client dead (seconds)
    #[serde(default = "default_ping_timeout_seconds")]
    pub ping_timeout_seconds: u64,
}

fn default_connection_rate_per_ip() -> u32 {
    3 // 3 connections per 10 seconds per IP (original hardcoded value)
}

fn default_join_rate_per_client() -> u32 {
    5 // 5 joins per 10 seconds per client (original hardcoded value)
}

fn default_ping_interval_seconds() -> u64 {
    90 // RFC2812 §3.7.2: servers SHOULD ping idle clients periodically
}

fn default_ping_timeout_seconds() -> u64 {
    30 // RFC2812 §3.7.2: reasonable grace period for pong replies
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    /// Log level filter
    pub level: String,

    /// Whether to include target in logs
    pub include_target: bool,

    /// Log format (compact, pretty, json)
    pub format: String,
}

/// Mass messaging (GLOBOPS, WALLOPS, PRIVMSG/NOTICE) permissions and flood control
/// Competitive analysis: All Big 4 have configurable mass message permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassMessageConfig {
    /// Enable/disable operator broadcast commands globally (GLOBOPS, LOCOPS, OPERWALL)
    #[serde(default = "default_mass_message_enabled")]
    pub enabled: bool,

    /// General PRIVMSG/NOTICE flood control: minimum permission level for mass messaging
    #[serde(default = "default_mass_message_min_permission")]
    pub min_permission_level: u32,

    /// General PRIVMSG/NOTICE flood control: maximum messages per window
    #[serde(default = "default_mass_message_flood_threshold")]
    pub flood_threshold: u32,

    /// Per-command minimum permission levels for operator broadcasts
    #[serde(default)]
    pub min_permission: MassMessagePermissions,

    /// Per-command rate limits (messages per minute) for operator broadcasts
    #[serde(default)]
    pub rate_limits: MassMessageRateLimits,
}

/// Per-command minimum permission levels for operator broadcast commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassMessagePermissions {
    /// Minimum permission for GLOBOPS (global operator notices)
    #[serde(default = "default_mass_message_permission")]
    pub globops: u32,

    /// Minimum permission for LOCOPS (local operator notices)
    #[serde(default = "default_mass_message_permission")]
    pub locops: u32,

    /// Minimum permission for OPERWALL (operator wall to +z users)
    #[serde(default = "default_mass_message_permission")]
    pub operwall: u32,
}

impl Default for MassMessagePermissions {
    fn default() -> Self {
        Self {
            globops: default_mass_message_permission(),
            locops: default_mass_message_permission(),
            operwall: default_mass_message_permission(),
        }
    }
}

/// Per-command rate limits for operator broadcast commands (messages per minute)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassMessageRateLimits {
    /// GLOBOPS messages per minute
    #[serde(default = "default_mass_message_rate")]
    pub globops_per_minute: u32,

    /// LOCOPS messages per minute
    #[serde(default = "default_mass_message_rate")]
    pub locops_per_minute: u32,

    /// OPERWALL messages per minute
    #[serde(default = "default_mass_message_rate")]
    pub operwall_per_minute: u32,
}

impl Default for MassMessageRateLimits {
    fn default() -> Self {
        Self {
            globops_per_minute: default_mass_message_rate(),
            locops_per_minute: default_mass_message_rate(),
            operwall_per_minute: default_mass_message_rate(),
        }
    }
}

impl Default for MassMessageConfig {
    fn default() -> Self {
        Self {
            enabled: default_mass_message_enabled(),
            min_permission_level: default_mass_message_min_permission(),
            flood_threshold: default_mass_message_flood_threshold(),
            min_permission: MassMessagePermissions::default(),
            rate_limits: MassMessageRateLimits::default(),
        }
    }
}

fn default_mass_message_enabled() -> bool {
    true
} // Enabled by default
fn default_mass_message_min_permission() -> u32 {
    1
} // Local operators for general mass messaging
fn default_mass_message_flood_threshold() -> u32 {
    5
} // 5 messages per window
fn default_mass_message_permission() -> u32 {
    500
} // Operator permission level for broadcasts
fn default_mass_message_rate() -> u32 {
    5
} // 5 messages per minute

/// Anti-spam content filtering configuration
///
/// CRITICAL DISTINCTION:
/// - Anti-abuse (network): Connection floods, join spam, rate limits (ALWAYS ON)
/// - Anti-spam (content): Message content filtering (OPTIONAL, configurable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiSpamSection {
    /// Enable spam content detection (keyword matching, entropy analysis, etc.)
    /// Default: false (opt-in for content filtering)
    #[serde(default = "default_anti_spam_enabled")]
    pub enabled: bool,

    /// Entropy threshold for gibberish detection (0.0-8.0)
    /// Lower = stricter detection, higher false positive rate
    /// Default: 3.0 (typical spam is <3.0, normal text >4.0)
    #[serde(default = "default_anti_spam_entropy_threshold")]
    pub entropy_threshold: f32,

    /// Maximum allowed character repetition before flagging as spam
    /// Default: 10 characters (e.g., "aaaaaaaaaa" triggers detection)
    #[serde(default = "default_anti_spam_max_repetition")]
    pub max_repetition: usize,

    /// Action to take on spam detection
    /// Options: "log" (default), "block", "silent"
    #[serde(default = "default_anti_spam_action")]
    pub action: String,
}

impl Default for AntiSpamSection {
    fn default() -> Self {
        Self {
            enabled: default_anti_spam_enabled(),
            entropy_threshold: default_anti_spam_entropy_threshold(),
            max_repetition: default_anti_spam_max_repetition(),
            action: default_anti_spam_action(),
        }
    }
}

fn default_anti_spam_enabled() -> bool {
    false // Opt-in for content filtering
}

fn default_anti_spam_entropy_threshold() -> f32 {
    3.0 // Tuned based on IRC spam corpus analysis
}

fn default_anti_spam_max_repetition() -> usize {
    10 // Characters before flagging as spam
}

fn default_anti_spam_action() -> String {
    "log".to_string() // Default: log only, don't block
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySection {
    /// DNS Blacklist (DNSBL) configuration
    pub dnsbl: DnsblSection,

    /// Anti-spam content filtering (optional, can be disabled)
    #[serde(default)]
    pub anti_spam: AntiSpamSection,

    /// MONITOR limit (IRCv3 extension)
    pub monitor_limit: usize,

    /// IRC Operator configuration
    #[serde(default)]
    pub operators: Vec<OperatorConfig>,

    /// Mass messaging (GLOBOPS, WALLOPS) configuration
    #[serde(default)]
    pub mass_message: MassMessageConfig,

    /// IRCv3 STS (Strict Transport Security) configuration
    #[serde(default)]
    pub sts: StsSection,
}

/// IRCv3 WEBIRC configuration for trusted gateway IP forwarding
/// https://ircv3.net/specs/extensions/webirc
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebircSection {
    /// Whether WEBIRC support is enabled
    pub enabled: bool,

    /// List of trusted gateways
    pub gateways: Vec<WebircGatewayConfig>,
}

/// Configuration for a trusted WEBIRC gateway
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebircGatewayConfig {
    /// Gateway identifier (e.g., "SLIRCdWeb")
    pub name: String,

    /// Shared secret password for authentication
    pub password: String,

    /// List of allowed IP addresses/CIDR ranges for this gateway
    pub allowed_ips: Vec<String>,

    /// Whether to require CAP negotiation before WEBIRC (optional, defaults to false)
    #[serde(default)]
    pub require_capability: bool,
}

/// IRC Operator credentials and permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorConfig {
    /// Operator username
    pub name: String,

    /// BCrypt hashed password (use `bcrypt-cli` or similar tool to generate)
    pub password_hash: String,

    /// Optional hostmask restriction (e.g., "*@192.168.1.*" or "*@trusted.host")
    /// If set, OPER only succeeds from matching hostmasks
    pub hostmask: Option<String>,

    /// Permission level: "local" (basic oper), "global" (network-wide), "admin" (full control)
    #[serde(default = "default_permission_level")]
    pub permission_level: String,
}

fn default_permission_level() -> String {
    "local".to_string()
}

/// Hot-reload configuration options
/// RUST ARCHITECT: Optional file watching for development convenience
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotReloadSection {
    /// Whether hot reload is enabled (default: true)
    #[serde(default = "default_hot_reload_enabled")]
    pub enabled: bool,

    /// Interval between config checks in seconds (default: 10)
    #[serde(default = "default_hot_reload_check_interval")]
    pub check_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloakingSection {
    /// Whether hostname cloaking is enabled (default: true)
    #[serde(default = "default_cloaking_enabled")]
    pub enabled: bool,

    /// Secret key for HMAC-SHA256 cloaking (MUST be kept private)
    /// Generate with: `openssl rand -base64 32` or `head -c 32 /dev/urandom | base64`
    /// SECURITY: Change this from default immediately in production
    /// Different keys produce different cloaks for same IP (prevents cross-server tracking)
    #[serde(default = "default_cloaking_secret_key")]
    pub secret_key: String,
}

impl Default for HotReloadSection {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 10,
        }
    }
}

/// Message history configuration for CHATHISTORY (IRCv3 draft/chathistory)
/// RFC: https://ircv3.net/specs/extensions/chathistory
///
/// FEATURES:
///   - Privacy-first: PM history disabled by default
///   - Rate limiting to prevent DoS abuse
///   - Flexible retention policy (days + message count limits)
///   - Configurable max messages per query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySection {
    /// Whether message history is enabled (default: true)
    #[serde(default = "default_history_enabled")]
    pub enabled: bool,

    /// Maximum messages per CHATHISTORY query (ISUPPORT token value)
    /// Client requests above this are clamped to this limit
    /// (default: 1000)
    #[serde(default = "default_chathistory_max")]
    pub chathistory_max: usize,

    /// Message retention in days (0 = forever, not recommended)
    /// Messages older than this are pruned by background task
    /// (default: 90)
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,

    /// Whether to store private message history (default: false)
    /// PRIVACY: PM history is privacy-sensitive and opt-in only
    /// When false, PM queries return empty batch (no error)
    #[serde(default = "default_store_private_messages")]
    pub store_private_messages: bool,

    /// Maximum CHATHISTORY queries per client per second
    /// Prevents DoS via repeated history requests
    /// IMPROVEMENT: Missing from all Big 4 implementations
    /// (default: 5)
    #[serde(default = "default_rate_limit_per_second")]
    pub rate_limit_per_second: u32,
}

impl Default for HistorySection {
    fn default() -> Self {
        Self {
            enabled: true,
            chathistory_max: 1000,
            retention_days: 90,
            store_private_messages: false,
            rate_limit_per_second: 5,
        }
    }
}

fn default_history_enabled() -> bool {
    true
}

fn default_chathistory_max() -> usize {
    1000
}

fn default_retention_days() -> u32 {
    90
}

fn default_store_private_messages() -> bool {
    false
}

fn default_rate_limit_per_second() -> u32 {
    5
}

/// Services (NickServ/ChanServ) configuration
/// Embedded services for nickname and channel registration
/// Competitive: Matches Ergo embedded model, simpler than Anope/Atheme separate process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicesSection {
    /// Whether services are enabled (default: false - opt-in for stability)
    #[serde(default)]
    pub enabled: bool,

    /// NickServ nickname (default: "NickServ")
    #[serde(default = "default_nickserv_name")]
    pub nickserv_name: String,

    /// ChanServ nickname (default: "ChanServ")
    #[serde(default = "default_chanserv_name")]
    pub chanserv_name: String,

    /// Nickname enforcement timeout in seconds (default: 30)
    /// Time before unidentified user is warned/kicked for using registered nick
    #[serde(default = "default_enforce_timeout")]
    pub enforce_timeout: u64,
}

impl Default for ServicesSection {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in for stability
            nickserv_name: default_nickserv_name(),
            chanserv_name: default_chanserv_name(),
            enforce_timeout: default_enforce_timeout(),
        }
    }
}

fn default_nickserv_name() -> String {
    "NickServ".to_string()
}

fn default_chanserv_name() -> String {
    "ChanServ".to_string()
}

fn default_enforce_timeout() -> u64 {
    30 // 30 seconds before enforcement warning
}

/// Metrics/monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSection {
    /// Metrics HTTP server bind address (default: "127.0.0.1:9090")
    #[serde(default = "default_metrics_bind_addr")]
    pub bind_addr: String,
}

impl Default for MetricsSection {
    fn default() -> Self {
        Self {
            bind_addr: default_metrics_bind_addr(),
        }
    }
}

fn default_metrics_bind_addr() -> String {
    "127.0.0.1:9090".to_string()
}

/// WebAdmin configuration - Web-based server management interface
/// Extends Prometheus metrics server with REST API + modern UI
/// Default: DISABLED for security (must opt-in)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAdminSection {
    /// Whether WebAdmin is enabled (default: false - MUST opt-in for security)
    #[serde(default)]
    pub enabled: bool,

    /// Admin username for HTTP Basic Auth (default: "admin")
    #[serde(default = "default_webadmin_username")]
    pub username: String,

    /// Admin password hash (bcrypt, generated via tools/genpasswd)
    /// MUST be bcrypt hash starting with $2b$
    /// Example: $2b$12$KIXx3q.vYf5dGNxF8j9.6OZq4Z7X8vYf5dGNxF8j9.6OZq4Z7X8vYf
    /// Default: None (WebAdmin won't work until password is set)
    pub password_hash: Option<String>,

    /// Maximum admin actions per minute (rate limiting, default: 10)
    /// Prevents brute force and abuse
    #[serde(default = "default_webadmin_rate_limit")]
    pub max_actions_per_minute: usize,

    /// Log all admin actions (default: true)
    /// Maintains audit trail with timestamps, IPs, and action details
    #[serde(default = "default_webadmin_log_actions")]
    pub log_all_actions: bool,

    /// Maximum event log entries to retain in memory (default: 1000)
    #[serde(default = "default_webadmin_max_log_entries")]
    pub max_log_entries: usize,
}

impl Default for WebAdminSection {
    fn default() -> Self {
        Self {
            enabled: false,  // SECURITY: Disabled by default
            username: default_webadmin_username(),
            password_hash: None,  // MUST be configured
            max_actions_per_minute: default_webadmin_rate_limit(),
            log_all_actions: default_webadmin_log_actions(),
            max_log_entries: default_webadmin_max_log_entries(),
        }
    }
}

fn default_webadmin_username() -> String {
    "admin".to_string()
}

fn default_webadmin_rate_limit() -> usize {
    10
}

fn default_webadmin_log_actions() -> bool {
    true
}

fn default_webadmin_max_log_entries() -> usize {
    1000
}

/// Server-to-server linking configuration (RFC2813)
/// Competitive: Adopts InspIRCd clarity + Solanum connection classes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingSection {
    /// Whether S2S linking is enabled (default: false - opt-in)
    #[serde(default)]
    pub enabled: bool,

    /// Server ID for UID generation (3-char alphanumeric, default: "001")
    /// RFC 2813 §2.3.1: UID format <SID><UID> for network-wide uniqueness
    /// TS6 protocol pattern (Solanum-compatible): 3-char SID + 6-char base36 UID
    /// Example: "001" generates UIDs like 001AAAAAA, 001AAAAAB, etc.
    /// MUST be unique across the network (enforced via CAPAB negotiation)
    #[serde(default = "default_server_id")]
    pub server_id: String,

    /// Dedicated S2S listener bind address (default: "0.0.0.0:7000")
    /// Separate from client ports for security isolation
    #[serde(default = "default_linking_bind")]
    pub bind: String,

    /// Connection class settings for all S2S links
    #[serde(default)]
    pub class: LinkingClass,

    /// Array of server link configurations
    #[serde(default)]
    pub servers: Vec<ServerLinkConfig>,
}

impl Default for LinkingSection {
    fn default() -> Self {
        Self {
            enabled: false,
            server_id: default_server_id(),
            bind: default_linking_bind(),
            class: LinkingClass::default(),
            servers: Vec::new(),
        }
    }
}

fn default_server_id() -> String {
    "001".to_string()
}

fn default_linking_bind() -> String {
    "0.0.0.0:7000".to_string()
}

/// Connection class for S2S links (rate limiting, keepalive)
/// Competitive: Matches Solanum connection class pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingClass {
    /// Ping interval in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_linking_ping_interval")]
    pub ping_interval_seconds: u64,

    /// Ping timeout in seconds (default: 90 = grace period after ping)
    #[serde(default = "default_linking_ping_timeout")]
    pub ping_timeout_seconds: u64,

    /// Send queue size in bytes (default: 2097152 = 2 MB)
    /// Higher than client sendq for burst tolerance
    #[serde(default = "default_linking_sendq")]
    pub sendq_bytes: usize,

    /// Maximum autoconnected servers (default: 1 - prevents topology loops)
    /// Set to 1 for hub-leaf model (leaf autoconnects to hub)
    #[serde(default = "default_linking_max_autoconn")]
    pub max_autoconn: u32,
}

impl Default for LinkingClass {
    fn default() -> Self {
        Self {
            ping_interval_seconds: default_linking_ping_interval(),
            ping_timeout_seconds: default_linking_ping_timeout(),
            sendq_bytes: default_linking_sendq(),
            max_autoconn: default_linking_max_autoconn(),
        }
    }
}

fn default_linking_ping_interval() -> u64 {
    300 // 5 minutes (Competitive: Solanum default)
}

fn default_linking_ping_timeout() -> u64 {
    90 // 90 seconds grace period
}

fn default_linking_sendq() -> usize {
    2_097_152 // 2 MB (Competitive: Solanum default)
}

fn default_linking_max_autoconn() -> u32 {
    1 // Prevent accidental topology loops
}

/// Individual server link configuration
/// Competitive: Asymmetric passwords (InspIRCd sendpass/recvpass pattern)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerLinkConfig {
    /// Remote server name (must match remote server's config name)
    pub name: String,

    /// Remote hostname or IP address
    pub host: String,

    /// Remote S2S port
    pub port: u16,

    /// Password we SEND to remote server (CRITICAL: asymmetric)
    /// Remote server's recv_password must match this
    pub send_password: String,

    /// Password we ACCEPT from remote server (CRITICAL: asymmetric)
    /// Remote server's send_password must match this
    pub recv_password: String,

    /// IP whitelist for incoming connections from this server
    /// Supports CIDR notation (e.g., "10.0.8.0/24") and wildcards (e.g., "192.168.1.*")
    pub allowed_ips: Vec<String>,

    /// Whether to automatically connect to this server (default: false)
    /// Hub servers: false (wait for leaf to connect)
    /// Leaf servers: true (initiate connection to hub)
    #[serde(default)]
    pub autoconnect: bool,

    /// Autoconnect retry delay in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_autoconnect_delay")]
    pub autoconnect_delay_seconds: u64,
}

fn default_autoconnect_delay() -> u64 {
    300 // 5 minutes retry delay
}

impl Default for CloakingSection {
    fn default() -> Self {
        Self {
            enabled: true,
            secret_key: default_cloaking_secret_key(),
        }
    }
}

fn default_hot_reload_enabled() -> bool {
    true
}

fn default_hot_reload_check_interval() -> u64 {
    10
}

fn default_cloaking_enabled() -> bool {
    true
}

fn default_cloaking_secret_key() -> String {
    // Generate a unique default key at runtime for new installations
    // Production deployments MUST override this in config.toml
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("slircd-default-key-{}-CHANGE-ME-IN-PRODUCTION", timestamp)
}

fn default_shutdown_delay_seconds() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsblSection {
    /// Whether DNSBL checking is enabled
    pub enabled: bool,

    /// List of DNSBL providers to query
    pub providers: Vec<String>,

    /// Timeout for DNS queries in milliseconds
    pub timeout_ms: u64,

    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,
}

/// IRCv3 STS (Strict Transport Security) configuration
/// https://ircv3.net/specs/extensions/sts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StsSection {
    /// Whether STS is enabled (only enable when TLS is properly configured)
    pub enabled: bool,

    /// TLS port for clients to upgrade to
    pub port: u16,

    /// Policy duration in seconds (how long clients remember to use TLS)
    pub duration: u32,
}

/// RFC 1459 §5 Optional Features Configuration
/// Disabled by default for security/privacy. Enable in trusted environments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionalFeaturesSection {
    /// RFC 1459 §5.4 SUMMON: Invite users to join IRC
    /// Modern: Sends server notice to IRC users (not shell users)
    /// Security: Requires oper privileges when enabled
    #[serde(default)]
    pub summon_enabled: bool,

    /// RFC 1459 §5.5 USERS: List users on server
    /// Modern: Lists IRC users (not shell users)
    /// Privacy: Returns registered IRC clients only
    #[serde(default = "default_users_max_results")]
    pub users_max_results: usize,

    /// RFC 1459 §5.5 USERS: Enable/disable feature
    #[serde(default)]
    pub users_enabled: bool,
}

fn default_users_max_results() -> usize {
    100
}

impl Default for OptionalFeaturesSection {
    fn default() -> Self {
        Self {
            summon_enabled: false, // Disabled by default (security)
            users_enabled: false,  // Disabled by default (privacy)
            users_max_results: default_users_max_results(),
        }
    }
}

impl Default for StsSection {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 6697,
            duration: 2592000, // 30 days
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerSection {
                name: "slircd".to_string(),
                motd: Some("motd.txt".to_string()),
                admin: AdminInfo {
                    location1: "SLIRCd IRC Server".to_string(),
                    location2: "Rust Implementation".to_string(),
                    email: "admin@localhost".to_string(),
                },
                server_password: None,
                shutdown_delay_seconds: default_shutdown_delay_seconds(),
                enforce_utf8: false, // Default: permissive mode for maximum compatibility
                whowas_retention_days: default_whowas_retention_days(), // Default: 7 days
            },
            listeners: ListenersSection {
                plaintext: ListenerConfig {
                    bind: "127.0.0.1:6667".to_string(),
                    enabled: true,
                },
                tls: None,
                websocket: None,
            },
            database: DatabaseSection {
                path: "slircd.db".to_string(),
                pool_size: 10,
                busy_timeout_ms: 5000,
            },
            tls: None,
            limits: LimitsSection {
                max_connections: None,
                max_connections_per_ip: None,
                connection_rate_per_minute: 100,
                message_rate_per_second: 10.0,
                connection_rate_per_ip: default_connection_rate_per_ip(),
                join_rate_per_client: default_join_rate_per_client(),
                ping_interval_seconds: default_ping_interval_seconds(),
                ping_timeout_seconds: default_ping_timeout_seconds(),
            },
            logging: LoggingSection {
                level: "slircd=info".to_string(),
                include_target: false,
                format: "compact".to_string(),
            },
            security: SecuritySection {
                dnsbl: DnsblSection {
                    enabled: false,
                    providers: vec![
                        "zen.spamhaus.org".to_string(),
                        "bl.spamcop.net".to_string(),
                        "cbl.abuseat.org".to_string(),
                    ],
                    timeout_ms: 2000,
                    cache_ttl_seconds: 3600,
                },
                anti_spam: AntiSpamSection::default(),
                monitor_limit: 100,
                operators: Vec::new(),
                mass_message: MassMessageConfig::default(),
                sts: StsSection::default(),
            },
            webirc: None, // Disabled by default for security
            hot_reload: HotReloadSection::default(),
            cloaking: CloakingSection::default(),
            history: HistorySection::default(),
            services: ServicesSection::default(),
            linking: LinkingSection::default(),
            metrics: MetricsSection::default(),
            webadmin: WebAdminSection::default(),
            optional_features: OptionalFeaturesSection::default(),
        }
    }
}

impl Config {
    /// Load configuration from TOML file with environment variable fallbacks
    pub async fn load_from_file<P: AsRef<Path>>(config_path: P) -> Result<Self> {
        let config_path = config_path.as_ref();

        if config_path.exists() {
            info!(?config_path, "loading configuration from file");
            let content = fs::read_to_string(config_path).await.with_context(|| {
                format!("failed to read config file: {}", config_path.display())
            })?;

            let mut config: Config = toml::from_str(&content).with_context(|| {
                format!("failed to parse config file: {}", config_path.display())
            })?;

            // Apply environment variable overrides
            config.apply_env_overrides()?;
            config.validate()?;

            Ok(config)
        } else {
            warn!(
                ?config_path,
                "config file not found, using environment variables and defaults"
            );
            Self::from_env()
        }
    }

    /// Create configuration from environment variables (legacy mode)
    pub fn from_env() -> Result<Self> {
        let mut config = Config::default();
        config.apply_env_overrides()?;
        config.validate()?;
        Ok(config)
    }

    /// Apply environment variable overrides to configuration
    ///
    /// Uses SLIRCD_* environment variables only.
    fn apply_env_overrides(&mut self) -> Result<()> {
        // Server configuration
        if let Ok(name) = std::env::var("SLIRCD_NAME") {
            self.server.name = name;
        }

        if let Ok(motd) = std::env::var("SLIRCD_MOTD") {
            self.server.motd = Some(motd);
        }

        if let Ok(password) = std::env::var("SLIRCD_SERVER_PASSWORD") {
            self.server.server_password = Some(password);
        }

        // Network binding
        if let Ok(bind) = std::env::var("SLIRCD_BIND") {
            self.listeners.plaintext.bind = bind;
        }

        // Database configuration
        if let Ok(db_path) = std::env::var("SLIRCD_DB") {
            self.database.path = db_path;
        }

        if let Ok(pool_size) = std::env::var("SLIRCD_DB_POOL_SIZE") {
            self.database.pool_size = pool_size
                .parse()
                .context("invalid SLIRCD_DB_POOL_SIZE value")?;
        }

        if let Ok(busy_ms) = std::env::var("SLIRCD_DB_BUSY_MS") {
            self.database.busy_timeout_ms =
                busy_ms.parse().context("invalid SLIRCD_DB_BUSY_MS value")?;
        }

        // TLS configuration
        let cert_path = std::env::var("SLIRCD_TLS_CERT").ok();
        let key_path = std::env::var("SLIRCD_TLS_KEY").ok();

        match (cert_path, key_path) {
            (Some(cert), Some(key)) => {
                self.tls = Some(TlsSection {
                    cert_path: cert,
                    key_path: key,
                });

                // Enable TLS listener if not already configured
                if self.listeners.tls.is_none() {
                    self.listeners.tls = Some(ListenerConfig {
                        bind: "127.0.0.1:6697".to_string(),
                        enabled: true,
                    });
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(anyhow!(
                    "Both SLIRCD_TLS_CERT and SLIRCD_TLS_KEY must be set"
                ));
            }
            (None, None) => {
                // No TLS configuration from environment
            }
        }

        // Connection limits
        if let Ok(max_conn) = std::env::var("SLIRCD_MAX_CONN") {
            self.limits.max_connections =
                Some(max_conn.parse().context("invalid SLIRCD_MAX_CONN value")?);
        }

        // WEBIRC configuration
        if let Ok(enabled) = std::env::var("SLIRCD_WEBIRC_ENABLED") {
            if enabled.to_lowercase() == "true" || enabled == "1" {
                // Only enable via env var if config doesn't already have it
                if self.webirc.is_none() {
                    self.webirc = Some(WebircSection {
                        enabled: true,
                        gateways: Vec::new(),
                    });
                }
            }
        }

        if let Ok(max_per_ip) = std::env::var("SLIRCD_MAX_CONN_PER_IP") {
            self.limits.max_connections_per_ip = Some(
                max_per_ip
                    .parse()
                    .context("invalid SLIRCD_MAX_CONN_PER_IP value")?,
            );
        }

        // Rate limit configuration via environment variables
        if let Ok(msg_rate) = std::env::var("SLIRCD_MESSAGE_RATE") {
            self.limits.message_rate_per_second = msg_rate
                .parse()
                .context("invalid SLIRCD_MESSAGE_RATE value")?;
        }

        if let Ok(conn_rate) = std::env::var("SLIRCD_CONN_RATE_PER_IP") {
            self.limits.connection_rate_per_ip = conn_rate
                .parse()
                .context("invalid SLIRCD_CONN_RATE_PER_IP value")?;
        }

        if let Ok(join_rate) = std::env::var("SLIRCD_JOIN_RATE") {
            self.limits.join_rate_per_client = join_rate
                .parse()
                .context("invalid SLIRCD_JOIN_RATE value")?;
        }

        if let Ok(ping_interval) = std::env::var("SLIRCD_PING_INTERVAL") {
            self.limits.ping_interval_seconds = ping_interval
                .parse()
                .context("invalid SLIRCD_PING_INTERVAL value")?;
        }

        if let Ok(ping_timeout) = std::env::var("SLIRCD_PING_TIMEOUT") {
            self.limits.ping_timeout_seconds = ping_timeout
                .parse()
                .context("invalid SLIRCD_PING_TIMEOUT value")?;
        }

        // Security configuration
        if let Ok(dnsbl_enabled) = std::env::var("SLIRCD_DNSBL_ENABLED") {
            self.security.dnsbl.enabled = dnsbl_enabled
                .parse::<bool>()
                .unwrap_or_else(|_| dnsbl_enabled.to_lowercase() == "true" || dnsbl_enabled == "1");
        }

        if let Ok(providers) = std::env::var("SLIRCD_DNSBL_PROVIDERS") {
            // Comma-separated list of DNSBL providers
            self.security.dnsbl.providers = providers
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        if let Ok(monitor_limit) = std::env::var("SLIRCD_MONITOR_LIMIT") {
            self.security.monitor_limit = monitor_limit
                .parse()
                .context("invalid SLIRCD_MONITOR_LIMIT value")?;
        }

        Ok(())
    }

    /// Validate configuration values
    /// RUST ARCHITECT: Comprehensive validation prevents runtime errors from bad config
    fn validate(&self) -> Result<()> {
        // Validate bind addresses
        self.listeners
            .plaintext
            .bind
            .parse::<SocketAddr>()
            .context("invalid plaintext bind address")?;

        if let Some(tls_listener) = &self.listeners.tls {
            tls_listener
                .bind
                .parse::<SocketAddr>()
                .context("invalid TLS bind address")?;
        }

        // Validate database configuration
        if self.database.pool_size == 0 {
            return Err(anyhow!("database pool_size must be greater than 0"));
        }

        if self.database.busy_timeout_ms == 0 {
            return Err(anyhow!("database busy_timeout_ms must be greater than 0"));
        }

        // Validate TLS configuration
        if let Some(tls) = &self.tls {
            if !Path::new(&tls.cert_path).exists() {
                return Err(anyhow!("TLS certificate file not found: {}", tls.cert_path));
            }
            if !Path::new(&tls.key_path).exists() {
                return Err(anyhow!("TLS key file not found: {}", tls.key_path));
            }
        }

        // Validate limits
        if self.limits.connection_rate_per_minute == 0 {
            return Err(anyhow!("connection_rate_per_minute must be greater than 0"));
        }

        if self.limits.message_rate_per_second <= 0.0 {
            return Err(anyhow!("message_rate_per_second must be greater than 0"));
        }

        if self.limits.ping_interval_seconds == 0 {
            return Err(anyhow!("ping_interval_seconds must be greater than 0"));
        }

        if self.limits.ping_timeout_seconds == 0 {
            return Err(anyhow!("ping_timeout_seconds must be greater than 0"));
        }

        if self.limits.ping_timeout_seconds >= self.limits.ping_interval_seconds {
            return Err(anyhow!(
                "ping_timeout_seconds must be less than ping_interval_seconds"
            ));
        }

        // Validate WEBIRC configuration (IRCv3 gateway IP forwarding)
        if let Some(webirc) = &self.webirc {
            if webirc.enabled && webirc.gateways.is_empty() {
                return Err(anyhow!("WEBIRC enabled but no gateways configured"));
            }

            for (idx, gateway) in webirc.gateways.iter().enumerate() {
                // Validate gateway name
                if gateway.name.is_empty() {
                    return Err(anyhow!("WEBIRC gateway[{}]: name cannot be empty", idx));
                }

                // Validate password
                if gateway.password.is_empty() {
                    return Err(anyhow!("WEBIRC gateway[{}]: password cannot be empty", idx));
                }

                // Validate allowed_ips list
                if gateway.allowed_ips.is_empty() {
                    return Err(anyhow!(
                        "WEBIRC gateway[{}] ({}): allowed_ips cannot be empty",
                        idx,
                        gateway.name
                    ));
                }

                // Validate each IP/CIDR format
                for ip_str in &gateway.allowed_ips {
                    self.validate_ip_pattern(ip_str).with_context(|| {
                        format!(
                            "WEBIRC gateway[{}] ({}): invalid IP pattern '{}'",
                            idx, gateway.name, ip_str
                        )
                    })?;
                }
            }
        }

        // Validate operator credentials
        for (idx, oper) in self.security.operators.iter().enumerate() {
            // Validate operator name
            if oper.name.is_empty() {
                return Err(anyhow!("operator[{}]: name cannot be empty", idx));
            }

            // Validate password hash format (should be bcrypt: $2b$...)
            if !oper.password_hash.starts_with("$2b$") && !oper.password_hash.starts_with("$2a$") {
                return Err(anyhow!(
                    "operator[{}] ({}): password_hash must be bcrypt format (starts with $2b$ or $2a$)",
                    idx,
                    oper.name
                ));
            }

            // Validate permission level
            match oper.permission_level.as_str() {
                "local" | "global" | "admin" => {}
                _ => {
                    return Err(anyhow!(
                        "operator[{}] ({}): invalid permission_level '{}' (must be: local, global, or admin)",
                        idx,
                        oper.name,
                        oper.permission_level
                    ));
                }
            }
        }

        // Validate DNSBL configuration
        if self.security.dnsbl.enabled {
            if self.security.dnsbl.providers.is_empty() {
                return Err(anyhow!("DNSBL enabled but no providers configured"));
            }

            if self.security.dnsbl.timeout_ms == 0 {
                return Err(anyhow!("DNSBL timeout_ms must be greater than 0"));
            }
        }

        // Validate services configuration
        // IRC SERVICES: NickServ/ChanServ require database persistence for account storage
        if self.services.enabled {
            // Services must have database configured (no in-memory-only mode)
            if self.database.path.is_empty() {
                return Err(anyhow!(
                    "services enabled but database path not configured (services require persistent storage)"
                ));
            }

            // Validate service bot nicknames (RFC 2812: NICK validation)
            if self.services.nickserv_name.is_empty() {
                return Err(anyhow!("services.nickserv_name cannot be empty"));
            }
            if self.services.chanserv_name.is_empty() {
                return Err(anyhow!("services.chanserv_name cannot be empty"));
            }

            // Enforce timeout must be reasonable (5-300 seconds)
            if self.services.enforce_timeout < 5 {
                return Err(anyhow!(
                    "services.enforce_timeout must be at least 5 seconds (too aggressive)"
                ));
            }
            if self.services.enforce_timeout > 300 {
                return Err(anyhow!(
                    "services.enforce_timeout must be at most 300 seconds (5 minutes max)"
                ));
            }
        }

        // Validate S2S linking configuration (Phase 6A: UID format)
        // RFC 2813 §2.3.1: UID format <SID><UID> requires 3-char server ID
        if self.linking.enabled {
            // Server ID must be exactly 3 characters (TS6 protocol)
            if self.linking.server_id.len() != 3 {
                return Err(anyhow!(
                    "linking.server_id must be exactly 3 characters (got: '{}' with length {})",
                    self.linking.server_id,
                    self.linking.server_id.len()
                ));
            }

            // Server ID must contain only alphanumeric characters (A-Z, 0-9)
            if !self
                .linking
                .server_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric())
            {
                return Err(anyhow!(
                    "linking.server_id must contain only alphanumeric characters (A-Z, 0-9), got: '{}'",
                    self.linking.server_id
                ));
            }

            // Server ID must be uppercase (normalize: 0ab → 0AB)
            let uppercase_sid = self.linking.server_id.to_uppercase();
            if self.linking.server_id != uppercase_sid {
                return Err(anyhow!(
                    "linking.server_id must be uppercase (use '{}' instead of '{}')",
                    uppercase_sid,
                    self.linking.server_id
                ));
            }

            // Validate server link configurations
            for (idx, link) in self.linking.servers.iter().enumerate() {
                if link.name.is_empty() {
                    return Err(anyhow!("linking.servers[{}]: name cannot be empty", idx));
                }
                if link.host.is_empty() {
                    return Err(anyhow!("linking.servers[{}]: host cannot be empty", idx));
                }
                if link.port == 0 {
                    return Err(anyhow!("linking.servers[{}]: port cannot be 0", idx));
                }
            }
        }

        Ok(())
    }

    /// Validate IP address pattern (IPv4, IPv6, CIDR notation, or wildcard pattern)
    /// SECURITY EXPERT: Validates WEBIRC whitelist entries for correct format
    /// COMPETITIVE ANALYSIS: UnrealIRCd uses security groups with CIDR support
    fn validate_ip_pattern(&self, pattern: &str) -> Result<()> {
        // Check for CIDR notation (e.g., "10.0.0.0/8", "2001:db8::/32")
        if pattern.contains('/') {
            use ipnetwork::IpNetwork;
            pattern
                .parse::<IpNetwork>()
                .context("invalid CIDR notation")?;
            return Ok(());
        }

        // Check for IPv4 address
        if pattern.parse::<std::net::Ipv4Addr>().is_ok() {
            return Ok(());
        }

        // Check for IPv6 address
        if pattern.parse::<std::net::Ipv6Addr>().is_ok() {
            return Ok(());
        }

        // Check for wildcard pattern (e.g., "192.168.1.*" or "10.*.*.*")
        if pattern.contains('*') {
            // Basic validation: should have 3 dots for IPv4 wildcard
            let parts: Vec<&str> = pattern.split('.').collect();
            if parts.len() == 4 {
                // Each part should be either a number or '*'
                for part in parts {
                    if part != "*" && part.parse::<u8>().is_err() {
                        return Err(anyhow!("invalid wildcard pattern segment: {}", part));
                    }
                }
                return Ok(());
            }
        }

        Err(anyhow!(
            "invalid IP pattern (must be IPv4, IPv6, wildcard like '192.168.1.*', or CIDR)"
        ))
    }

    /// Get the primary bind address (plaintext listener)
    pub fn bind_addr(&self) -> Result<SocketAddr> {
        self.listeners
            .plaintext
            .bind
            .parse()
            .context("invalid bind address")
    }

    /// Get TLS bind address if TLS is enabled
    pub fn tls_bind_addr(&self) -> Result<Option<SocketAddr>> {
        if let Some(tls_listener) = &self.listeners.tls {
            if tls_listener.enabled {
                return Ok(Some(
                    tls_listener
                        .bind
                        .parse()
                        .context("invalid TLS bind address")?,
                ));
            }
        }
        Ok(None)
    }

    /// Check if TLS is enabled
    pub fn tls_enabled(&self) -> bool {
        self.tls.is_some() && self.listeners.tls.as_ref().is_some_and(|l| l.enabled)
    }

    /// Get database connection timeout
    pub fn database_timeout(&self) -> Duration {
        Duration::from_millis(self.database.busy_timeout_ms as u64)
    }

    /// Generate example configuration file content
    pub fn example_toml() -> String {
        let config = Config::default();
        toml::to_string_pretty(&config)
            .unwrap_or_else(|e| format!("# Error generating example config: {}", e))
    }
}
