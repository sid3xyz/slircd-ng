//! Multiclient/bouncer configuration.
//!
//! This module defines configuration for the bouncer functionality,
//! including multiclient connections, always-on persistence, and auto-away.

use serde::Deserialize;

/// Multiclient configuration for bouncer features.
///
/// Controls how multiple connections can share the same account/nick,
/// whether users persist when disconnected (always-on), and related settings.
#[derive(Debug, Clone, Deserialize)]
pub struct MulticlientConfig {
    /// Enable multiclient support (multiple connections to same nick).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Allow multiclient by default, or require opt-in via NickServ SET.
    #[serde(default = "default_true")]
    pub allowed_by_default: bool,

    /// Always-on mode policy.
    /// - "disabled": Never persist clients when disconnected
    /// - "opt-in": Users can enable via NickServ SET always-on true
    /// - "opt-out": Enabled by default, users can disable
    /// - "mandatory": Always persist, users cannot disable
    #[serde(default)]
    pub always_on: AlwaysOnPolicy,

    /// How long to keep always-on clients before expiring (0 = forever).
    /// Format: "30d", "7d", "24h", "0" for never
    #[serde(default = "default_always_on_expiration")]
    pub always_on_expiration: String,

    /// Auto-away policy: set away when all sessions disconnect.
    /// - "disabled": Never set auto-away
    /// - "opt-in": Users can enable via NickServ SET auto-away true
    /// - "opt-out": Enabled by default, users can disable
    #[serde(default)]
    pub auto_away: AutoAwayPolicy,

    /// Maximum concurrent sessions per account (DoS protection).
    #[serde(default = "default_max_sessions")]
    pub max_sessions_per_account: usize,
}

/// Always-on persistence policy.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AlwaysOnPolicy {
    /// Never persist clients when disconnected.
    Disabled,
    /// Users must opt-in via NickServ SET always-on true.
    #[default]
    OptIn,
    /// Enabled by default, users can opt-out.
    OptOut,
    /// Always persist, users cannot disable.
    Mandatory,
}

/// Auto-away policy.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AutoAwayPolicy {
    /// Never set auto-away.
    Disabled,
    /// Users must opt-in via NickServ SET auto-away true.
    OptIn,
    /// Enabled by default, users can opt-out.
    #[default]
    OptOut,
}

impl Default for MulticlientConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_by_default: true,
            always_on: AlwaysOnPolicy::default(),
            always_on_expiration: default_always_on_expiration(),
            auto_away: AutoAwayPolicy::default(),
            max_sessions_per_account: default_max_sessions(),
        }
    }
}

impl MulticlientConfig {
    /// Check if multiclient is enabled for a given account.
    ///
    /// Takes the per-account setting (from NickServ) if set, otherwise uses config default.
    pub fn is_multiclient_enabled(&self, account_setting: Option<bool>) -> bool {
        if !self.enabled {
            return false;
        }
        account_setting.unwrap_or(self.allowed_by_default)
    }

    /// Check if always-on should be enabled for a given account.
    ///
    /// Takes the per-account setting (from NickServ) if set, otherwise uses policy default.
    pub fn is_always_on_enabled(&self, account_setting: Option<bool>) -> bool {
        match self.always_on {
            AlwaysOnPolicy::Disabled => false,
            AlwaysOnPolicy::Mandatory => true,
            AlwaysOnPolicy::OptIn => account_setting.unwrap_or(false),
            AlwaysOnPolicy::OptOut => account_setting.unwrap_or(true),
        }
    }

    /// Check if auto-away should be enabled for a given account.
    ///
    /// Takes the per-account setting (from NickServ) if set, otherwise uses policy default.
    pub fn is_auto_away_enabled(&self, account_setting: Option<bool>) -> bool {
        match self.auto_away {
            AutoAwayPolicy::Disabled => false,
            AutoAwayPolicy::OptIn => account_setting.unwrap_or(false),
            AutoAwayPolicy::OptOut => account_setting.unwrap_or(true),
        }
    }

    /// Parse the always-on expiration into a chrono::Duration.
    ///
    /// Returns None for "0" (never expire), or the parsed duration.
    pub fn parse_expiration(&self) -> Option<chrono::Duration> {
        parse_duration_string(&self.always_on_expiration)
    }
}

fn default_true() -> bool {
    true
}

fn default_always_on_expiration() -> String {
    "30d".to_string()
}

fn default_max_sessions() -> usize {
    10
}

/// Parse a duration string like "30d", "7d", "24h", "0" into chrono::Duration.
///
/// Returns None for "0" or invalid format.
pub fn parse_duration_string(s: &str) -> Option<chrono::Duration> {
    let s = s.trim();
    if s == "0" || s.is_empty() {
        return None;
    }

    let (num_str, unit) = if let Some(stripped) = s.strip_suffix('d') {
        (stripped, 'd')
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 'h')
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 'm')
    } else {
        // Assume days if no unit
        (s, 'd')
    };

    let num: i64 = num_str.parse().ok()?;
    match unit {
        'd' => Some(chrono::Duration::days(num)),
        'h' => Some(chrono::Duration::hours(num)),
        'm' => Some(chrono::Duration::minutes(num)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = MulticlientConfig::default();
        assert!(config.enabled);
        assert!(config.allowed_by_default);
        assert_eq!(config.always_on, AlwaysOnPolicy::OptIn);
        assert_eq!(config.auto_away, AutoAwayPolicy::OptOut);
        assert_eq!(config.max_sessions_per_account, 10);
        assert_eq!(config.always_on_expiration, "30d");
    }

    #[test]
    fn multiclient_disabled() {
        let config = MulticlientConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(!config.is_multiclient_enabled(None));
        assert!(!config.is_multiclient_enabled(Some(true)));
    }

    #[test]
    fn multiclient_with_account_override() {
        let config = MulticlientConfig::default();

        // Default is allowed
        assert!(config.is_multiclient_enabled(None));

        // User can opt-out
        assert!(!config.is_multiclient_enabled(Some(false)));

        // User can explicitly opt-in
        assert!(config.is_multiclient_enabled(Some(true)));
    }

    #[test]
    fn always_on_policies() {
        let disabled = MulticlientConfig {
            always_on: AlwaysOnPolicy::Disabled,
            ..Default::default()
        };
        assert!(!disabled.is_always_on_enabled(None));
        assert!(!disabled.is_always_on_enabled(Some(true)));

        let mandatory = MulticlientConfig {
            always_on: AlwaysOnPolicy::Mandatory,
            ..Default::default()
        };
        assert!(mandatory.is_always_on_enabled(None));
        assert!(mandatory.is_always_on_enabled(Some(false)));

        let opt_in = MulticlientConfig {
            always_on: AlwaysOnPolicy::OptIn,
            ..Default::default()
        };
        assert!(!opt_in.is_always_on_enabled(None));
        assert!(opt_in.is_always_on_enabled(Some(true)));

        let opt_out = MulticlientConfig {
            always_on: AlwaysOnPolicy::OptOut,
            ..Default::default()
        };
        assert!(opt_out.is_always_on_enabled(None));
        assert!(!opt_out.is_always_on_enabled(Some(false)));
    }

    #[test]
    fn auto_away_policies() {
        let disabled = MulticlientConfig {
            auto_away: AutoAwayPolicy::Disabled,
            ..Default::default()
        };
        assert!(!disabled.is_auto_away_enabled(None));

        let opt_in = MulticlientConfig {
            auto_away: AutoAwayPolicy::OptIn,
            ..Default::default()
        };
        assert!(!opt_in.is_auto_away_enabled(None));
        assert!(opt_in.is_auto_away_enabled(Some(true)));

        let opt_out = MulticlientConfig {
            auto_away: AutoAwayPolicy::OptOut,
            ..Default::default()
        };
        assert!(opt_out.is_auto_away_enabled(None));
        assert!(!opt_out.is_auto_away_enabled(Some(false)));
    }

    #[test]
    fn parse_duration() {
        assert!(parse_duration_string("0").is_none());
        assert!(parse_duration_string("").is_none());

        let d = parse_duration_string("30d").unwrap();
        assert_eq!(d.num_days(), 30);

        let h = parse_duration_string("24h").unwrap();
        assert_eq!(h.num_hours(), 24);

        let m = parse_duration_string("60m").unwrap();
        assert_eq!(m.num_minutes(), 60);

        // No unit defaults to days
        let no_unit = parse_duration_string("7").unwrap();
        assert_eq!(no_unit.num_days(), 7);
    }
}
