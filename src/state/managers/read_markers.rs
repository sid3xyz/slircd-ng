//! Read markers manager for per-device, per-target last-seen tracking.
//!
//! Tracks the last delivered message timestamp (nanoseconds) for each
//! (account, device_id, target) triple. Used to bound autoreplay ranges
//! and avoid duplicating history across reconnects.

use dashmap::DashMap;
use slirc_proto::irc_to_lower;

/// Key for a read marker entry: (account_lower, device_id, target_lower)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadMarkerKey {
    pub account: String,
    pub device_id: String,
    pub target: String,
}

/// Read markers manager.
#[derive(Default)]
pub struct ReadMarkersManager {
    markers: DashMap<ReadMarkerKey, i64>, // nanotime
}

impl ReadMarkersManager {
    /// Set/update a marker for the given account/device/target to `nanotime`.
    pub fn set(&self, account: &str, device_id: &str, target: &str, nanotime: i64) {
        let key = ReadMarkerKey {
            account: irc_to_lower(account),
            device_id: device_id.to_string(),
            target: irc_to_lower(target),
        };
        self.markers.insert(key, nanotime);
    }

    /// Get the marker nanotime for the given account/device/target.
    pub fn get(&self, account: &str, device_id: &str, target: &str) -> Option<i64> {
        let key = ReadMarkerKey {
            account: irc_to_lower(account),
            device_id: device_id.to_string(),
            target: irc_to_lower(target),
        };
        self.markers.get(&key).map(|v| *v.value())
    }
}
