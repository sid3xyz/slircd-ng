//! Read markers manager for unified read state tracking.
//!
//! Tracks the last read message timestamp (nanoseconds) for each
//! (account, target) pair. Used to synchronize read state across
//! devices (IRCv3 `draft/read-marker`).
//!
//! Persistence is handled via `AlwaysOnStore` (Redb).

use crate::db::always_on::AlwaysOnStore;
use dashmap::DashMap;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tracing::{error, warn};

/// Key for a read marker entry: (account_lower, target_lower)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadMarkerKey {
    pub account: String,
    pub target: String,
}

/// Read markers manager.
pub struct ReadMarkersManager {
    /// In-memory cache of markers.
    markers: DashMap<ReadMarkerKey, i64>, // nanotime
    /// Persistent storage (optional).
    store: Option<Arc<AlwaysOnStore>>,
}

impl ReadMarkersManager {
    /// Create a new ReadMarkersManager.
    pub fn new(store: Option<Arc<AlwaysOnStore>>) -> Self {
        Self {
            markers: DashMap::new(),
            store,
        }
    }

    /// Set/update a marker for the given account/target to `nanotime`.
    ///
    /// Persists to storage if configured.
    pub fn update_marker(&self, account: &str, target: &str, nanotime: i64) {
        let key = ReadMarkerKey {
            account: irc_to_lower(account),
            target: irc_to_lower(target),
        };

        // Update in-memory
        self.markers.insert(key.clone(), nanotime);

        // Persist
        if let Some(store) = &self.store 
            && let Err(e) = store.save_read_marker(&key.account, &key.target, nanotime) 
        {
            error!(account = %key.account, target = %key.target, error = %e, "Failed to persist read marker");
        }
    }

    /// Get the marker nanotime for the given account/target.
    ///
    /// Checks in-memory cache first, then storage.
    pub fn get_marker(&self, account: &str, target: &str) -> Option<i64> {
        let key = ReadMarkerKey {
            account: irc_to_lower(account),
            target: irc_to_lower(target),
        };

        // Check cache
        if let Some(v) = self.markers.get(&key) {
            return Some(*v.value());
        }

        // Check storage
        if let Some(store) = &self.store {
            match store.get_read_marker(&key.account, &key.target) {
                Ok(Some(ts)) => {
                    // Cache it
                    self.markers.insert(key, ts);
                    return Some(ts);
                }
                Ok(None) => return None,
                Err(e) => {
                    warn!(account = %key.account, target = %key.target, error = %e, "Failed to load read marker");
                    return None;
                }
            }
        }

        None
    }

    /// Cleanup markers for a deleted account.
    #[allow(dead_code)]
    pub fn cleanup_account(&self, account: &str) {
        let account_lower = irc_to_lower(account);
        
        // Remove from cache
        self.markers.retain(|k, _| k.account != account_lower);

        // Remove from storage
        if let Some(store) = &self.store 
            && let Err(e) = store.delete_markers(&account_lower) 
        {
            error!(account = %account_lower, error = %e, "Failed to delete read markers");
        }
    }
}

impl Default for ReadMarkersManager {
    fn default() -> Self {
        Self::new(None)
    }
}
