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
        }
    }
}

fn default_history_backend() -> String {
    "none".to_string()
}

fn default_history_path() -> String {
    "history.db".to_string()
}
