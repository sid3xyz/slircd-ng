//! Default value functions for configuration.
//!
//! Separated into its own module for clarity and reuse.

use rand::distributions::Alphanumeric;
use rand::Rng;

/// Returns `true` (for serde defaults).
pub fn default_true() -> bool {
    true
}

// =============================================================================
// History Defaults
// =============================================================================

pub fn default_history_backend() -> String {
    "none".to_string()
}

pub fn default_history_path() -> String {
    "history.db".to_string()
}

// =============================================================================
// Idle Timeout Defaults
// =============================================================================

pub fn default_ping_interval() -> u64 {
    90
}

pub fn default_ping_timeout() -> u64 {
    120
}

pub fn default_registration_timeout() -> u64 {
    60
}

// =============================================================================
// Heuristics Defaults
// =============================================================================

pub fn default_velocity_window() -> u64 {
    10
}

pub fn default_max_velocity() -> usize {
    5
}

pub fn default_fanout_window() -> u64 {
    60
}

pub fn default_max_fanout() -> usize {
    10
}

pub fn default_repetition_decay() -> f32 {
    0.8
}

// =============================================================================
// Security Defaults
// =============================================================================

pub fn default_cloak_secret() -> String {
    let secret: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    tracing::warn!(
        "No cloak_secret configured - using ephemeral random secret. \
         Cloaked hostnames will NOT be consistent across server restarts. \
         Set [security].cloak_secret in config.toml for production use."
    );
    secret
}

pub fn default_cloak_suffix() -> String {
    "ip".to_string()
}

pub fn default_spam_detection_enabled() -> bool {
    true
}

// =============================================================================
// Limits Defaults
// =============================================================================

pub fn default_max_who_results() -> usize {
    500
}

pub fn default_max_list_channels() -> usize {
    1000
}

pub fn default_max_names_channels() -> usize {
    50
}

pub fn default_channel_mailbox_capacity() -> usize {
    500
}

// =============================================================================
// Rate Limit Defaults
// =============================================================================

pub fn default_message_rate() -> u32 {
    2
}

pub fn default_connection_burst() -> u32 {
    3
}

pub fn default_join_burst() -> u32 {
    5
}

pub fn default_ctcp_rate() -> u32 {
    1
}

pub fn default_ctcp_burst() -> u32 {
    2
}

pub fn default_max_connections() -> u32 {
    10
}
