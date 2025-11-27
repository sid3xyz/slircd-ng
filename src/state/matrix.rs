//! The Matrix - Central shared state for the IRC server.
//!
//! The Matrix holds all users, channels, and server state in concurrent
//! data structures accessible from any async task.

use crate::config::ServerConfig;
use crate::state::UidGenerator;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Unique user identifier (TS6 format: 9 characters).
pub type Uid = String;

/// Server identifier (TS6 format: 3 characters).
pub type Sid = String;

/// The Matrix - Central shared state container.
///
/// This is the core state of the IRC server, holding all users, channels,
/// and related data in thread-safe concurrent collections.
pub struct Matrix {
    /// All connected users, indexed by UID.
    pub users: DashMap<Uid, Arc<RwLock<User>>>,

    /// All channels, indexed by lowercase name.
    pub channels: DashMap<String, Arc<RwLock<Channel>>>,

    /// Nick to UID mapping for fast nick lookups.
    pub nicks: DashMap<String, Uid>,

    /// Connected servers (for future linking support).
    pub servers: DashMap<Sid, Arc<Server>>,

    /// This server's identity.
    pub server_info: ServerInfo,

    /// UID generator for new connections.
    pub uid_gen: UidGenerator,
}

/// This server's identity information.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub sid: Sid,
    pub name: String,
    pub network: String,
    pub description: String,
    pub created: i64,
}

/// A connected user.
#[derive(Debug)]
pub struct User {
    pub uid: Uid,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub host: String,
}

/// An IRC channel.
#[derive(Debug)]
pub struct Channel {
    pub name: String,
    pub topic: Option<String>,
    pub created: i64,
}

/// A linked server (for future use).
#[derive(Debug)]
pub struct Server {
    pub sid: Sid,
    pub name: String,
    pub description: String,
}

impl Matrix {
    /// Create a new Matrix with the given server configuration.
    pub fn new(config: &ServerConfig) -> Self {
        let now = chrono::Utc::now().timestamp();

        Self {
            users: DashMap::new(),
            channels: DashMap::new(),
            nicks: DashMap::new(),
            servers: DashMap::new(),
            server_info: ServerInfo {
                sid: config.sid.clone(),
                name: config.name.clone(),
                network: config.network.clone(),
                description: config.description.clone(),
                created: now,
            },
            uid_gen: UidGenerator::new(config.sid.clone()),
        }
    }
}
