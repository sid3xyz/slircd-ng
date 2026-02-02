use crate::db::Database;
use dashmap::DashMap;

/// Manages read markers (Unified Read State) for users.
///
/// Tracks the last read timestamp for a user (account) in a given target (channel/query).
pub struct ReadMarkerManager {
    /// In-memory cache of markers: (account, target) -> timestamp (nanos)
    /// Key format: "account:target" or we could use tuple.
    /// Using a simple string key map for now for simplicity.
    /// Key: `account_name:target_name` (both lowercased)
    markers: DashMap<String, i64>,

    /// Database handle for persistence (TODO: Implement persistence)
    #[allow(dead_code)]
    db: Option<Database>,
}

impl ReadMarkerManager {
    /// Create a new ReadMarkerManager.
    pub fn new(db: Option<Database>) -> Self {
        Self {
            markers: DashMap::new(),
            db,
        }
    }

    /// Update the read marker for an account in a target.
    pub fn update_marker(&self, account: &str, target: &str, timestamp: i64) {
        let key = format!("{}:{}", account.to_lowercase(), target.to_lowercase());
        // Only update if newer? Or just overwrite?
        // Usually we only advance read markers forward.
        // But for now, simple overwrite or max.
        
        self.markers.entry(key)
            .and_modify(|ts| *ts = (*ts).max(timestamp))
            .or_insert(timestamp);
            
        // TODO: Persist to DB asynchronously
    }

    /// Get the read marker for an account in a target.
    pub fn get_marker(&self, account: &str, target: &str) -> Option<i64> {
        let key = format!("{}:{}", account.to_lowercase(), target.to_lowercase());
        self.markers.get(&key).map(|v| *v)
    }
}
