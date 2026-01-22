//! MAP command handler.
//!
//! `MAP`
//!
//! Returns the server map (network topology).

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use crate::sync::{ServerInfo, TopologyGraph};
use async_trait::async_trait;
use slirc_proto::sync::clock::ServerId;
use slirc_proto::{MessageRef, Response};

/// Handler for MAP command.
pub struct MapHandler;

#[async_trait]
impl PostRegHandler for MapHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let nick = &ctx.state.nick;

        // Helper to collect children of a given server SID
        // Returns owned ServerInfo to avoid keeping DashMap locks (entry guards) alive
        fn collect_children(
            topology: &TopologyGraph,
            parent_sid: &ServerId,
        ) -> Vec<ServerInfo> {
            let mut children: Vec<ServerInfo> = topology
                .servers
                .iter()
                .filter_map(|entry| {
                    let info = entry.value();
                    if let Some(via) = &info.via {
                        if via == parent_sid {
                            return Some(info.clone());
                        }
                    }
                    None
                })
                .collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            children
        }

        // Recursive depth‑first traversal producing lines
        fn traverse(
            topology: &TopologyGraph,
            current_sid: &ServerId,
            prefix: String,
            is_last: bool,
            lines: &mut Vec<String>,
        ) {
            // Find current server info in topology
            // If it's the local server, it should be in the topology.
            // We clone to release the lock immediately.
            let info_opt = topology.servers.get(current_sid).map(|r| r.clone());

            if let Some(info) = info_opt {
                let connector = if prefix.is_empty() {
                    "`-".to_string()
                } else if is_last {
                    format!("{}`-", prefix)
                } else {
                    format!("{}|-", prefix)
                };
                
                // TODO: Include user count if available/synced
                lines.push(format!("{} {}", connector, info.name));

                // Prepare prefix for children
                let child_prefix = if prefix.is_empty() {
                    "   ".to_string()
                } else if is_last {
                    format!("{}   ", prefix)
                } else {
                    format!("{}|  ", prefix)
                };

                let children = collect_children(topology, current_sid);
                let count = children.len();
                for (idx, child) in children.iter().enumerate() {
                    let child_is_last = idx == count - 1;
                    traverse(
                        topology,
                        &child.sid,
                        child_prefix.clone(),
                        child_is_last,
                        lines,
                    );
                }
            }
        }

        let mut map_lines: Vec<String> = Vec::new();
        // Use server_id (ServerId) instead of server_info.sid (String)
        let local_sid = &ctx.matrix.server_id;
        
        traverse(
            &ctx.matrix.sync_manager.topology,
            local_sid,
            String::new(),
            true,
            &mut map_lines,
        );

        // Send each line as a separate RPL_MAP reply
        for line in map_lines {
            ctx.send_reply(Response::RPL_MAP, vec![nick.clone(), line]).await?;
        }

        // End of MAP
        ctx.send_reply(
            Response::RPL_MAPEND,
            vec![nick.clone(), "End of MAP".to_string()],
        )
        .await?;

        Ok(())
    }
}
