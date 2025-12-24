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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_correct() {
        let config = LimitsConfig::default();
        assert_eq!(config.max_who_results, 500);
        assert_eq!(config.max_list_channels, 1000);
        assert_eq!(config.max_names_channels, 50);
        assert_eq!(config.channel_mailbox_capacity, 500);
    }

    #[test]
    fn default_max_who_results_returns_500() {
        assert_eq!(default_max_who_results(), 500);
    }

    #[test]
    fn default_max_list_channels_returns_1000() {
        assert_eq!(default_max_list_channels(), 1000);
    }

    #[test]
    fn default_max_names_channels_returns_50() {
        assert_eq!(default_max_names_channels(), 50);
    }

    #[test]
    fn default_channel_mailbox_capacity_returns_500() {
        assert_eq!(default_channel_mailbox_capacity(), 500);
    }

    #[test]
    fn limits_config_is_clone() {
        let config = LimitsConfig::default();
        let cloned = config.clone();
        assert_eq!(cloned.max_who_results, config.max_who_results);
        assert_eq!(cloned.max_list_channels, config.max_list_channels);
    }

    #[test]
    fn limits_config_debug_impl() {
        let config = LimitsConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("LimitsConfig"));
        assert!(debug_str.contains("500")); // max_who_results
    }
}
