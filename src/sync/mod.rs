//! Sync Module - Server-to-Server Synchronization.
//!
//! This module manages the distributed state of the IRC network.
//! It handles server linking, handshake, and CRDT state replication.

pub mod burst;
pub mod handshake;
pub mod link;
pub mod manager;
pub mod network;
mod observer;
pub mod split;
pub mod stream;
pub mod tls;
mod topology;

#[cfg(test)]
mod tests;

// Re-export topology types
pub use link::LinkState;
pub use manager::SyncManager;
pub use topology::TopologyGraph;
pub use topology::ServerInfo;
