//! History storage configuration (Innovation 5: Event-Sourced History).

use serde::Deserialize;

use super::types::default_true;

/// History configuration (Innovation 5: Event-Sourced History).
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryConfig {
    /// Whether history is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Backend type: "redb", "sqlite", "none".
    #[serde(default = "default_history_backend")]
    pub backend: String,
    /// Path to history database file.
    #[serde(default = "default_history_path")]
    pub path: String,
    /// Maximum number of messages to return for ZNC `play <channel> <start>` form.
    /// Defaults to 50 if not set.
    #[serde(default, rename = "znc-maxmessages")]
    pub znc_maxmessages: Option<usize>,
    /// Event type configuration.
    #[serde(default)]
    pub events: HistoryEventsConfig,
}

/// Configuration for which event types to store in history.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryEventsConfig {
    /// Store PRIVMSG messages.
    #[serde(default = "default_true")]
    pub privmsg: bool,
    /// Store NOTICE messages.
    #[serde(default = "default_true")]
    pub notice: bool,
    /// Store TOPIC changes (requires event-playback to replay).
    #[serde(default = "default_true")]
    pub topic: bool,
    /// Store TAGMSG (only with +draft/persist tag, requires event-playback).
    #[serde(default = "default_true")]
    pub tagmsg: bool,
    /// Store JOIN/PART/QUIT events (future, requires event-playback).
    #[serde(default)]
    pub membership: bool,
    /// Store MODE changes (future, requires event-playback).
    #[serde(default)]
    pub mode: bool,
}

impl Default for HistoryEventsConfig {
    fn default() -> Self {
        Self {
            privmsg: true,
            notice: true,
            topic: true,
            tagmsg: true,
            membership: false,
            mode: false,
        }
    }
}

impl HistoryConfig {
    /// Check if a specific event type should be stored.
    pub fn should_store_event(&self, event_type: &str) -> bool {
        if !self.enabled {
            return false;
        }
        match event_type {
            "PRIVMSG" => self.events.privmsg,
            "NOTICE" => self.events.notice,
            "TOPIC" => self.events.topic,
            "TAGMSG" => self.events.tagmsg,
            "JOIN" | "PART" | "QUIT" | "KICK" => self.events.membership,
            "MODE" => self.events.mode,
            _ => false,
        }
    }
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: "none".to_string(),
            path: "history.db".to_string(),
            events: HistoryEventsConfig::default(),
            znc_maxmessages: None,
        }
    }
}

fn default_history_backend() -> String {
    "none".to_string()
}

fn default_history_path() -> String {
    "history.db".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // === HistoryEventsConfig Default Tests ===

    #[test]
    fn history_events_config_default_privmsg_enabled() {
        let config = HistoryEventsConfig::default();
        assert!(config.privmsg, "privmsg should default to true");
    }

    #[test]
    fn history_events_config_default_notice_enabled() {
        let config = HistoryEventsConfig::default();
        assert!(config.notice, "notice should default to true");
    }

    #[test]
    fn history_events_config_default_topic_enabled() {
        let config = HistoryEventsConfig::default();
        assert!(config.topic, "topic should default to true");
    }

    #[test]
    fn history_events_config_default_tagmsg_enabled() {
        let config = HistoryEventsConfig::default();
        assert!(config.tagmsg, "tagmsg should default to true");
    }

    #[test]
    fn history_events_config_default_membership_disabled() {
        let config = HistoryEventsConfig::default();
        assert!(!config.membership, "membership should default to false");
    }

    #[test]
    fn history_events_config_default_mode_disabled() {
        let config = HistoryEventsConfig::default();
        assert!(!config.mode, "mode should default to false");
    }

    // === HistoryConfig Default Tests ===

    #[test]
    fn history_config_default_disabled() {
        let config = HistoryConfig::default();
        assert!(!config.enabled, "history should default to disabled");
    }

    #[test]
    fn history_config_default_backend_none() {
        let config = HistoryConfig::default();
        assert_eq!(config.backend, "none");
    }

    #[test]
    fn history_config_default_path() {
        let config = HistoryConfig::default();
        assert_eq!(config.path, "history.db");
    }

    // === should_store_event Tests ===

    #[test]
    fn should_store_event_returns_false_when_disabled() {
        let config = HistoryConfig {
            enabled: false,
            events: HistoryEventsConfig {
                privmsg: true,
                notice: true,
                topic: true,
                tagmsg: true,
                membership: true,
                mode: true,
            },
            ..Default::default()
        };
        // Even with all events enabled, disabled config returns false
        assert!(!config.should_store_event("PRIVMSG"));
        assert!(!config.should_store_event("NOTICE"));
        assert!(!config.should_store_event("TOPIC"));
        assert!(!config.should_store_event("TAGMSG"));
        assert!(!config.should_store_event("JOIN"));
        assert!(!config.should_store_event("PART"));
        assert!(!config.should_store_event("QUIT"));
        assert!(!config.should_store_event("KICK"));
        assert!(!config.should_store_event("MODE"));
    }

    #[test]
    fn should_store_event_privmsg_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                privmsg: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("PRIVMSG"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                privmsg: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("PRIVMSG"));
    }

    #[test]
    fn should_store_event_notice_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                notice: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("NOTICE"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                notice: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("NOTICE"));
    }

    #[test]
    fn should_store_event_topic_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                topic: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("TOPIC"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                topic: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("TOPIC"));
    }

    #[test]
    fn should_store_event_tagmsg_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                tagmsg: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("TAGMSG"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                tagmsg: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("TAGMSG"));
    }

    #[test]
    fn should_store_event_membership_events_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                membership: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("JOIN"));
        assert!(enabled_config.should_store_event("PART"));
        assert!(enabled_config.should_store_event("QUIT"));
        assert!(enabled_config.should_store_event("KICK"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                membership: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("JOIN"));
        assert!(!disabled_config.should_store_event("PART"));
        assert!(!disabled_config.should_store_event("QUIT"));
        assert!(!disabled_config.should_store_event("KICK"));
    }

    #[test]
    fn should_store_event_mode_based_on_config() {
        let enabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                mode: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(enabled_config.should_store_event("MODE"));

        let disabled_config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                mode: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!disabled_config.should_store_event("MODE"));
    }

    #[test]
    fn should_store_event_unknown_event_returns_false() {
        let config = HistoryConfig {
            enabled: true,
            events: HistoryEventsConfig {
                privmsg: true,
                notice: true,
                topic: true,
                tagmsg: true,
                membership: true,
                mode: true,
            },
            ..Default::default()
        };
        // Unknown event types always return false
        assert!(!config.should_store_event("UNKNOWN"));
        assert!(!config.should_store_event("PING"));
        assert!(!config.should_store_event("PONG"));
        assert!(!config.should_store_event("WHO"));
        assert!(!config.should_store_event(""));
    }

    // === Helper function default tests ===

    #[test]
    fn default_history_backend_is_none() {
        assert_eq!(default_history_backend(), "none");
    }

    #[test]
    fn default_history_path_is_history_db() {
        assert_eq!(default_history_path(), "history.db");
    }
}
