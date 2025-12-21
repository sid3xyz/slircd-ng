//! Network topology tracking for distributed IRC.
//!
//! Tracks the spanning tree of connected servers to enable efficient
//! routing and netsplit detection.

use dashmap::DashMap;
use slirc_crdt::clock::ServerId;
use std::collections::HashSet;

/// Information about a server in the network.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used for network introspection and future features
pub struct ServerInfo {
    /// Server ID (3-character unique identifier).
    pub sid: ServerId,
    /// Human-readable server name.
    pub name: String,
    /// Server description/info string.
    pub info: String,
    /// Number of hops from this server.
    pub hopcount: u32,
    /// The SID of the server we route through to reach this one.
    /// `None` for our own server.
    pub via: Option<ServerId>,
}

/// Tracks the network topology as a spanning tree.
///
/// The topology graph tracks all servers in the network and the routing
/// paths to reach them. This enables:
/// - Efficient message routing
/// - Netsplit detection and cleanup
/// - Network map generation
#[derive(Debug, Clone)]
pub struct TopologyGraph {
    /// All servers in the network, keyed by SID.
    pub servers: DashMap<ServerId, ServerInfo>,
}

impl TopologyGraph {
    /// Create a new empty topology graph.
    pub fn new() -> Self {
        Self {
            servers: DashMap::new(),
        }
    }

    /// Register a server in the topology.
    ///
    /// # Arguments
    /// * `sid` - The server's unique identifier
    /// * `name` - The server's name
    /// * `info` - The server's description
    /// * `hopcount` - Hops to reach this server
    /// * `via` - The SID we route through to reach this server (None for direct peers)
    #[allow(dead_code)] // Part of public API for topology management
    pub fn add_server(
        &self,
        sid: ServerId,
        name: String,
        info: String,
        hopcount: u32,
        via: Option<ServerId>,
    ) {
        self.servers.insert(
            sid.clone(),
            ServerInfo {
                sid,
                name,
                info,
                hopcount,
                via,
            },
        );
    }

    /// Remove a server from the topology.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn remove_server(&self, sid: &ServerId) -> Option<ServerInfo> {
        self.servers.remove(sid).map(|(_, info)| info)
    }

    /// Check if a server exists in the topology.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn contains(&self, sid: &ServerId) -> bool {
        self.servers.contains_key(sid)
    }

    /// Get information about a server.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn get(&self, sid: &ServerId) -> Option<ServerInfo> {
        self.servers.get(sid).map(|r| r.clone())
    }

    /// Get the SID we route through to reach a target server.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn get_route(&self, target: &ServerId) -> Option<ServerId> {
        self.servers.get(target).and_then(|info| info.via.clone())
    }

    /// Get all SIDs that are downstream of a given server.
    ///
    /// This returns the target SID and all SIDs that route *through* it.
    /// Used during netsplit cleanup to find all affected servers.
    ///
    /// # Algorithm
    /// Traverse the topology and collect all servers whose `via` field
    /// matches the target SID, then recursively collect their downstream
    /// servers as well.
    pub fn get_downstream_sids(&self, target_sid: &ServerId) -> Vec<ServerId> {
        let mut result = Vec::new();
        let mut to_process = vec![target_sid.clone()];
        let mut processed = HashSet::new();

        while let Some(current) = to_process.pop() {
            if processed.contains(&current) {
                continue;
            }
            processed.insert(current.clone());
            result.push(current.clone());

            // Find all servers that route through 'current'
            for entry in self.servers.iter() {
                let info = entry.value();
                if let Some(via) = &info.via
                    && via == &current && !processed.contains(&info.sid)
                {
                    to_process.push(info.sid.clone());
                }
            }
        }

        result
    }

    /// Get all server SIDs in the topology.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn all_sids(&self) -> Vec<ServerId> {
        self.servers.iter().map(|e| e.key().clone()).collect()
    }

    /// Get the number of servers in the topology.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn len(&self) -> usize {
        self.servers.len()
    }

    /// Check if the topology is empty.
    #[allow(dead_code)] // Part of public API for topology management
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Remove multiple servers from the topology.
    pub fn remove_servers(&self, sids: &[ServerId]) {
        for sid in sids {
            self.servers.remove(sid);
        }
    }
}

impl Default for TopologyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downstream_sids_linear() {
        // Linear topology: Local -> A -> B -> C
        let graph = TopologyGraph::new();
        
        let local = ServerId::new("001".to_string());
        let a = ServerId::new("00A".to_string());
        let b = ServerId::new("00B".to_string());
        let c = ServerId::new("00C".to_string());

        graph.add_server(local.clone(), "local".to_string(), "".to_string(), 0, None);
        graph.add_server(a.clone(), "serverA".to_string(), "".to_string(), 1, Some(local.clone()));
        graph.add_server(b.clone(), "serverB".to_string(), "".to_string(), 2, Some(a.clone()));
        graph.add_server(c.clone(), "serverC".to_string(), "".to_string(), 3, Some(b.clone()));

        // If A disconnects, we lose A, B, and C
        let downstream = graph.get_downstream_sids(&a);
        assert!(downstream.contains(&a));
        assert!(downstream.contains(&b));
        assert!(downstream.contains(&c));
        assert!(!downstream.contains(&local));
        assert_eq!(downstream.len(), 3);
    }

    #[test]
    fn test_downstream_sids_tree() {
        // Tree topology: Local -> A -> B
        //                      -> C
        let graph = TopologyGraph::new();
        
        let local = ServerId::new("001".to_string());
        let a = ServerId::new("00A".to_string());
        let b = ServerId::new("00B".to_string());
        let c = ServerId::new("00C".to_string());

        graph.add_server(local.clone(), "local".to_string(), "".to_string(), 0, None);
        graph.add_server(a.clone(), "serverA".to_string(), "".to_string(), 1, Some(local.clone()));
        graph.add_server(b.clone(), "serverB".to_string(), "".to_string(), 2, Some(a.clone()));
        graph.add_server(c.clone(), "serverC".to_string(), "".to_string(), 1, Some(local.clone()));

        // If A disconnects, we lose A and B, but not C
        let downstream = graph.get_downstream_sids(&a);
        assert!(downstream.contains(&a));
        assert!(downstream.contains(&b));
        assert!(!downstream.contains(&c));
        assert!(!downstream.contains(&local));
        assert_eq!(downstream.len(), 2);
    }

    #[test]
    fn test_downstream_sids_leaf() {
        // If we disconnect a leaf node, only that node is affected
        let graph = TopologyGraph::new();
        
        let local = ServerId::new("001".to_string());
        let a = ServerId::new("00A".to_string());

        graph.add_server(local.clone(), "local".to_string(), "".to_string(), 0, None);
        graph.add_server(a.clone(), "serverA".to_string(), "".to_string(), 1, Some(local.clone()));

        let downstream = graph.get_downstream_sids(&a);
        assert_eq!(downstream.len(), 1);
        assert!(downstream.contains(&a));
    }
}
