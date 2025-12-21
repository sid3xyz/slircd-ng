//! Monitor management state.
//!
//! This module contains the `MonitorManager` struct, which isolates all
//! MONITOR-related state from the main Matrix struct.

use crate::state::Uid;
use dashmap::{DashMap, DashSet};

/// Monitor management state.
///
/// The MonitorManager holds all MONITOR-related state, including:
/// - Forward mapping: UIDs to monitored nicknames
/// - Reverse mapping: nicknames to monitoring UIDs
pub struct MonitorManager {
    /// MONITOR: Nicknames being monitored by each UID.
    /// Key is UID, value is set of lowercase nicknames.
    pub monitors: DashMap<Uid, DashSet<String>>,

    /// MONITOR: Reverse mapping - who is monitoring each nickname.
    /// Key is lowercase nickname, value is set of UIDs monitoring it.
    pub monitoring: DashMap<String, DashSet<Uid>>,
}

impl MonitorManager {
    /// Create a new MonitorManager.
    pub fn new() -> Self {
        Self {
            monitors: DashMap::new(),
            monitoring: DashMap::new(),
        }
    }
}
