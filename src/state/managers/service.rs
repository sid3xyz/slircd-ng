//! Service management state and behavior.
//!
//! This module contains the `ServiceManager` struct, which isolates all
//! service-related state and logic from the main Matrix struct.

use crate::db::Database;
use crate::history::HistoryProvider;
use crate::services::{Service, chanserv, nickserv};
use std::collections::HashMap;
use std::sync::Arc;

/// Service management state.
///
/// The ServiceManager holds all service-related state, including:
/// - NickServ for nickname registration and identification
/// - ChanServ for channel registration and access control
/// - Extra services for dynamic service loading
/// - History provider for message history
pub struct ServiceManager {
    /// NickServ service singleton.
    pub nickserv: nickserv::NickServ,

    /// ChanServ service singleton.
    pub chanserv: chanserv::ChanServ,

    /// Message history provider (Opt-In Hybrid Architecture).
    pub history: Arc<dyn HistoryProvider>,

    /// Extra services (dynamic).
    pub extra_services: HashMap<String, Box<dyn Service>>,
}

impl ServiceManager {
    /// Create a new ServiceManager with the given database.
    pub fn new(db: Database, history: Arc<dyn HistoryProvider>) -> Self {
        Self {
            nickserv: nickserv::NickServ::new(db.clone()),
            chanserv: chanserv::ChanServ::new(db),
            history,
            extra_services: HashMap::new(),
        }
    }
}
