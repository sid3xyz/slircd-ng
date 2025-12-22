//! Service management state and behavior.
//!
//! This module contains the `ServiceManager` struct, which isolates all
//! service-related state and logic from the main Matrix struct.

use crate::db::Database;
use crate::history::HistoryProvider;
use crate::services::{Service, chanserv, nickserv};
use crate::state::{User, UserModes};
use slirc_crdt::clock::HybridTimestamp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Well-known UID suffix for NickServ (always AAAAAA within the server's SID).
pub const NICKSERV_UID_SUFFIX: &str = "AAAAAA";
/// Well-known UID suffix for ChanServ (always AAAAAB within the server's SID).
pub const CHANSERV_UID_SUFFIX: &str = "AAAAAB";

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

    /// UID for NickServ (set during initialization).
    pub nickserv_uid: String,

    /// UID for ChanServ (set during initialization).
    pub chanserv_uid: String,
}

impl ServiceManager {
    /// Create a new ServiceManager with the given database and server SID.
    pub fn new(db: Database, history: Arc<dyn HistoryProvider>, server_sid: &str) -> Self {
        let nickserv_uid = format!("{}{}", server_sid, NICKSERV_UID_SUFFIX);
        let chanserv_uid = format!("{}{}", server_sid, CHANSERV_UID_SUFFIX);

        Self {
            nickserv: nickserv::NickServ::new(db.clone()),
            chanserv: chanserv::ChanServ::new(db),
            history,
            extra_services: HashMap::new(),
            nickserv_uid,
            chanserv_uid,
        }
    }

    /// Create User structs for service pseudoclients.
    ///
    /// These users are registered in UserManager so they appear in BURST
    /// and can receive messages from remote servers.
    pub fn create_service_users(
        &self,
        server_name: &str,
        server_id: &slirc_crdt::clock::ServerId,
    ) -> Vec<User> {
        let now = HybridTimestamp::now(server_id);

        vec![
            User {
                uid: self.nickserv_uid.clone(),
                nick: "NickServ".to_string(),
                user: "services".to_string(),
                realname: "Nickname Registration Service".to_string(),
                host: server_name.to_string(),
                ip: "0.0.0.0".to_string(),
                visible_host: server_name.to_string(),
                session_id: Uuid::nil(), // Services don't have real sessions
                channels: HashSet::new(),
                modes: UserModes {
                    service: true,
                    registered: true,
                    ..Default::default()
                },
                account: Some("NickServ".to_string()),
                away: None,
                caps: HashSet::new(),
                certfp: None,
                silence_list: HashSet::new(),
                accept_list: HashSet::new(),
                last_modified: now,
            },
            User {
                uid: self.chanserv_uid.clone(),
                nick: "ChanServ".to_string(),
                user: "services".to_string(),
                realname: "Channel Registration Service".to_string(),
                host: server_name.to_string(),
                ip: "0.0.0.0".to_string(),
                visible_host: server_name.to_string(),
                session_id: Uuid::nil(),
                channels: HashSet::new(),
                modes: UserModes {
                    service: true,
                    registered: true,
                    ..Default::default()
                },
                account: Some("ChanServ".to_string()),
                away: None,
                caps: HashSet::new(),
                certfp: None,
                silence_list: HashSet::new(),
                accept_list: HashSet::new(),
                last_modified: now,
            },
        ]
    }

    /// Check if a UID belongs to a service.
    pub fn is_service_uid(&self, uid: &str) -> bool {
        uid == self.nickserv_uid || uid == self.chanserv_uid
    }

    /// Get service name by UID.
    #[allow(dead_code)]
    pub fn get_service_name(&self, uid: &str) -> Option<&'static str> {
        if uid == self.nickserv_uid {
            Some("NickServ")
        } else if uid == self.chanserv_uid {
            Some("ChanServ")
        } else {
            None
        }
    }
}
