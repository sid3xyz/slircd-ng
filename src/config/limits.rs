//! Command output limits configuration.

use serde::Deserialize;

/// Command output limits configuration.
///
/// These limits prevent pathologically large result sets from exhausting
/// server resources or causing slow clients to back up.
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    /// Maximum results returned by WHO command (default: 500).
    /// Applies to both channel WHO and mask-based WHO queries.
    #[serde(default = "default_max_who_results")]
    pub max_who_results: usize,
    /// Maximum channels returned by LIST command (default: 1000).
    #[serde(default = "default_max_list_channels")]
    pub max_list_channels: usize,
    /// Maximum channels listed by NAMES without argument (default: 50).
    /// NAMES #channel is unlimited since it's a single channel.
    #[serde(default = "default_max_names_channels")]
    pub max_names_channels: usize,
    /// Channel actor mailbox capacity (default: 500).
    /// Higher values provide burst tolerance during floods.
    #[serde(default = "default_channel_mailbox_capacity")]
    pub channel_mailbox_capacity: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_who_results: default_max_who_results(),
            max_list_channels: default_max_list_channels(),
            max_names_channels: default_max_names_channels(),
            channel_mailbox_capacity: default_channel_mailbox_capacity(),
        }
    }
}

fn default_max_who_results() -> usize {
    500
}

fn default_max_list_channels() -> usize {
    1000
}

fn default_max_names_channels() -> usize {
    50
}

fn default_channel_mailbox_capacity() -> usize {
    500
}
