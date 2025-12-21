use serde::{Deserialize, Serialize};
use slirc_crdt::clock::HybridTimestamp;

/// Tombstone for a destroyed channel.
///
/// Prevents "zombie" channels from being resurrected by delayed merge events
/// from other servers that haven't seen the destruction yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ChannelTombstone {
    pub name: String,
    pub deleted_at: HybridTimestamp,
}
