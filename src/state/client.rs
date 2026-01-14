//! Client state for bouncer/multiclient support.
//!
//! A Client represents an account's persistent state, which can have zero or more
//! attached Sessions (connections). This enables:
//!
//! - **Multiclient**: Multiple connections sharing the same nick/channels
//! - **Always-on**: Persistence when all connections disconnect
//! - **History playback**: Per-device last-seen for missed message tracking
//!
//! # Architecture
//!
//! ```text
//! Connection₁ ─┐
//! Connection₂ ─┼→ Client (Account) → Virtual Presence (User)
//! Connection₃ ─┘     1                      1
//! ```
//!
//! Unlike Ergo's MySQL-based approach, we use Redb for persistence and CRDT
//! for cross-server synchronization.

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use uuid::Uuid;

use crate::state::UserModes;

/// Unique identifier for a connection/session.
pub type SessionId = Uuid;

/// Unique identifier for a client device (extracted from SASL username or ident).
pub type DeviceId = String;

/// Represents an account's persistent bouncer state.
///
/// A Client can have 0..N attached Sessions. When all Sessions disconnect,
/// the Client can optionally persist (always-on mode) and rejoin when a new
/// Session connects.
#[derive(Debug)]
pub struct Client {
    /// Account name (primary identifier, casefolded).
    pub account: String,

    /// Current nickname (may be same as account in nick-equals-account mode).
    pub nick: String,

    /// Currently attached session IDs (0 when always-on but disconnected).
    pub sessions: HashSet<SessionId>,

    /// Whether always-on mode is enabled for this account.
    pub always_on: bool,

    /// Whether auto-away is enabled (set away when all sessions disconnect).
    pub auto_away: bool,

    /// Current away message (if set).
    pub away: Option<String>,

    /// User modes (persisted for always-on).
    pub modes: UserModes,

    /// Channels and per-channel membership info.
    pub channels: HashMap<String, ChannelMembership>,

    /// Per-device last-seen timestamps (for history playback).
    pub last_seen: HashMap<DeviceId, DateTime<Utc>>,

    /// Registered devices with metadata.
    pub devices: HashMap<DeviceId, DeviceInfo>,

    /// When this client was created.
    pub created_at: DateTime<Utc>,

    /// Dirty bits for efficient persistence writeback.
    dirty_bits: AtomicU32,
}

/// Dirty bits for selective persistence.
pub mod dirty {
    pub const CHANNELS: u32 = 1 << 0;
    pub const NICK: u32 = 1 << 1;
    pub const AWAY: u32 = 1 << 2;
    pub const LAST_SEEN: u32 = 1 << 3;
    pub const DEVICES: u32 = 1 << 4;
    pub const ALWAYS_ON: u32 = 1 << 5;
}

/// Information about a client's channel membership.
#[derive(Debug, Clone)]
pub struct ChannelMembership {
    /// Channel modes for this user (e.g., "ov" for +o +v).
    pub modes: String,

    /// When the user joined this channel (Unix timestamp).
    pub join_time: i64,
}

/// Information about a registered device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device identifier.
    pub id: DeviceId,

    /// Human-readable device name.
    pub name: Option<String>,

    /// TLS certificate fingerprint bound to this device (optional).
    pub certfp: Option<String>,

    /// When this device was first seen.
    pub created_at: DateTime<Utc>,

    /// When this device was last seen.
    pub last_seen: DateTime<Utc>,
}

impl Client {
    /// Create a new Client for an account.
    pub fn new(account: String, nick: String) -> Self {
        Self {
            account,
            nick,
            sessions: HashSet::new(),
            always_on: false,
            auto_away: false,
            away: None,
            modes: UserModes::default(),
            channels: HashMap::new(),
            last_seen: HashMap::new(),
            devices: HashMap::new(),
            created_at: Utc::now(),
            dirty_bits: AtomicU32::new(0),
        }
    }

    /// Attach a session to this client.
    ///
    /// Returns `true` if successfully attached, `false` if the session was already attached.
    pub fn attach_session(&mut self, session_id: SessionId) -> bool {
        let inserted = self.sessions.insert(session_id);
        if inserted {
            // Clear auto-away when a session connects
            if self.auto_away && self.away.is_some() {
                self.away = None;
                self.mark_dirty(dirty::AWAY);
            }
        }
        inserted
    }

    /// Detach a session from this client.
    ///
    /// Returns `true` if the session was attached and is now removed.
    pub fn detach_session(&mut self, session_id: SessionId) -> bool {
        let removed = self.sessions.remove(&session_id);
        if removed && self.sessions.is_empty() && self.auto_away {
            // Set auto-away when last session disconnects
            self.away = Some("Auto-away: All sessions disconnected".to_string());
            self.mark_dirty(dirty::AWAY);
        }
        removed
    }

    /// Check if this client has any connected sessions.
    pub fn is_connected(&self) -> bool {
        !self.sessions.is_empty()
    }

    /// Get the number of attached sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Update the last-seen timestamp for a device.
    pub fn update_last_seen(&mut self, device_id: &DeviceId) {
        self.last_seen.insert(device_id.clone(), Utc::now());
        self.mark_dirty(dirty::LAST_SEEN);
    }

    /// Get the last-seen timestamp for a device.
    pub fn get_last_seen(&self, device_id: &DeviceId) -> Option<DateTime<Utc>> {
        self.last_seen.get(device_id).copied()
    }

    /// Join a channel with the given modes.
    pub fn join_channel(&mut self, channel: &str, modes: &str) {
        let channel_lower = slirc_proto::irc_to_lower(channel);
        self.channels.insert(
            channel_lower,
            ChannelMembership {
                modes: modes.to_string(),
                join_time: Utc::now().timestamp(),
            },
        );
        self.mark_dirty(dirty::CHANNELS);
    }

    /// Part from a channel.
    pub fn part_channel(&mut self, channel: &str) {
        let channel_lower = slirc_proto::irc_to_lower(channel);
        if self.channels.remove(&channel_lower).is_some() {
            self.mark_dirty(dirty::CHANNELS);
        }
    }

    /// Register a new device.
    pub fn register_device(&mut self, device_id: DeviceId, name: Option<String>) -> &DeviceInfo {
        let now = Utc::now();
        let is_new = !self.devices.contains_key(&device_id);
        self.devices
            .entry(device_id.clone())
            .or_insert_with(|| DeviceInfo {
                id: device_id.clone(),
                name,
                certfp: None,
                created_at: now,
                last_seen: now,
            });
        if is_new {
            self.mark_dirty(dirty::DEVICES);
        }
        self.devices.get(&device_id).expect("just inserted")
    }

    /// Update a device's last-seen timestamp.
    pub fn touch_device(&mut self, device_id: &DeviceId) {
        if let Some(device) = self.devices.get_mut(device_id) {
            device.last_seen = Utc::now();
            self.mark_dirty(dirty::DEVICES);
        }
    }

    /// Set the always-on mode for this client.
    pub fn set_always_on(&mut self, enabled: bool) {
        if self.always_on != enabled {
            self.always_on = enabled;
            self.mark_dirty(dirty::ALWAYS_ON);
        }
    }

    /// Set the auto-away mode for this client.
    pub fn set_auto_away(&mut self, enabled: bool) {
        if self.auto_away != enabled {
            self.auto_away = enabled;
            self.mark_dirty(dirty::ALWAYS_ON);
        }
    }

    /// Mark specific dirty bits for persistence.
    pub fn mark_dirty(&self, bits: u32) {
        self.dirty_bits.fetch_or(bits, Ordering::Relaxed);
    }

    /// Read and clear dirty bits.
    pub fn take_dirty(&self) -> u32 {
        self.dirty_bits.swap(0, Ordering::Relaxed)
    }
}

/// Session attachment information for routing.
#[derive(Debug, Clone)]
pub struct SessionAttachment {
    /// The session's unique ID.
    pub session_id: SessionId,

    /// Device ID for this session (if known).
    pub device_id: Option<DeviceId>,

    /// Account this session is attached to.
    pub account: String,

    /// IP address of the session.
    pub ip: String,

    /// When this session attached.
    pub attached_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creation() {
        let client = Client::new("alice".to_string(), "Alice".to_string());
        assert_eq!(client.account, "alice");
        assert_eq!(client.nick, "Alice");
        assert!(!client.is_connected());
        assert!(!client.always_on);
    }

    #[test]
    fn session_attach_detach() {
        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        let session1 = SessionId::new_v4();
        let session2 = SessionId::new_v4();

        // Attach first session
        assert!(client.attach_session(session1));
        assert!(client.is_connected());
        assert_eq!(client.session_count(), 1);

        // Attach second session
        assert!(client.attach_session(session2));
        assert_eq!(client.session_count(), 2);

        // Duplicate attach returns false
        assert!(!client.attach_session(session1));
        assert_eq!(client.session_count(), 2);

        // Detach first session
        assert!(client.detach_session(session1));
        assert!(client.is_connected());
        assert_eq!(client.session_count(), 1);

        // Detach second session
        assert!(client.detach_session(session2));
        assert!(!client.is_connected());
        assert_eq!(client.session_count(), 0);

        // Double detach returns false
        assert!(!client.detach_session(session1));
    }

    #[test]
    fn auto_away_on_disconnect() {
        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        client.set_auto_away(true);
        let session = SessionId::new_v4();

        // Attach session - no away
        client.attach_session(session);
        assert!(client.away.is_none());

        // Detach session - auto-away set
        client.detach_session(session);
        assert!(client.away.is_some());
        assert!(client.away.as_ref().unwrap().contains("Auto-away"));
    }

    #[test]
    fn auto_away_cleared_on_connect() {
        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        client.set_auto_away(true);
        client.away = Some("Auto-away: All sessions disconnected".to_string());

        // Attach session - away cleared
        client.attach_session(SessionId::new_v4());
        assert!(client.away.is_none());
    }

    #[test]
    fn channel_operations() {
        let mut client = Client::new("alice".to_string(), "Alice".to_string());

        // Join channel
        client.join_channel("#Test", "ov");
        assert!(client.channels.contains_key("#test"));
        assert_eq!(client.channels.get("#test").unwrap().modes, "ov");

        // Part channel
        client.part_channel("#test");
        assert!(!client.channels.contains_key("#test"));
    }

    #[test]
    fn device_registration() {
        let mut client = Client::new("alice".to_string(), "Alice".to_string());

        // Register device
        let device = client.register_device("phone".to_string(), Some("iPhone".to_string()));
        assert_eq!(device.id, "phone");
        assert_eq!(device.name, Some("iPhone".to_string()));

        // Touch device
        let old_seen = client.devices.get("phone").unwrap().last_seen;
        std::thread::sleep(std::time::Duration::from_millis(10));
        client.touch_device(&"phone".to_string());
        assert!(client.devices.get("phone").unwrap().last_seen > old_seen);
    }

    #[test]
    fn dirty_bits() {
        let client = Client::new("alice".to_string(), "Alice".to_string());
        assert_eq!(client.dirty_bits.load(Ordering::Relaxed), 0);

        client.mark_dirty(dirty::CHANNELS | dirty::AWAY);
        assert_ne!(client.dirty_bits.load(Ordering::Relaxed), 0);

        let bits = client.take_dirty();
        assert_eq!(bits, dirty::CHANNELS | dirty::AWAY);
        assert_eq!(client.dirty_bits.load(Ordering::Relaxed), 0);
    }
}
