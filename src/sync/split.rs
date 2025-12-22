//! Netsplit handling for distributed IRC.
//!
//! When a server link drops, this module handles the cleanup:
//! - Identifies all servers that became unreachable
//! - Performs a "mass quit" for all affected users
//! - Updates the topology graph
//! - Notifies local clients of the splits

use crate::state::Matrix;
use slirc_crdt::clock::ServerId;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tracing::{debug, info};

/// Handle a netsplit when a server link drops.
///
/// This function:
/// 1. Calculates all servers that became unreachable
/// 2. Removes all users from those servers
/// 3. Updates the topology graph
/// 4. Notifies local clients via QUIT messages
///
/// # Arguments
/// * `matrix` - The server state matrix
/// * `dead_link_sid` - The SID of the server that disconnected
/// * `local_name` - Local server name for the quit message
/// * `remote_name` - Remote server name for the quit message
pub async fn handle_netsplit(
    matrix: &Matrix,
    dead_link_sid: &ServerId,
    local_name: &str,
    remote_name: &str,
) {
    info!(
        dead_sid = %dead_link_sid.as_str(),
        "Netsplit detected, calculating affected scope"
    );

    // 1. Calculate all SIDs that are now unreachable
    let affected_sids = matrix
        .sync_manager
        .topology
        .get_downstream_sids(dead_link_sid);

    if affected_sids.is_empty() {
        debug!(
            "No servers affected by netsplit from {}",
            dead_link_sid.as_str()
        );
        return;
    }

    info!(
        affected_count = affected_sids.len(),
        "Netsplit affects {} server(s)",
        affected_sids.len()
    );

    // Build the quit reason
    let quit_reason = format!("{} {}", local_name, remote_name);

    // 2. Mass quit: Find and remove all users from affected servers
    let mut affected_users = Vec::new();

    // Collect affected users (users whose UID starts with an affected SID)
    for entry in matrix.user_manager.users.iter() {
        let uid = entry.key();

        // Extract SID from UID (first 3 characters)
        if uid.len() >= 3 {
            let user_sid = ServerId::new(uid[0..3].to_string());
            if affected_sids.contains(&user_sid) {
                affected_users.push(uid.clone());
            }
        }
    }

    info!(
        user_count = affected_users.len(),
        "Mass quit: {} user(s) affected",
        affected_users.len()
    );

    // 3. Process each affected user
    for uid in &affected_users {
        // Get user info before removal for QUIT message
        let quit_msg = if let Some(user_arc) = matrix.user_manager.users.get(uid) {
            let user = user_arc.read().await;
            let nick = user.nick.clone();
            let user_str = user.user.clone();
            let host = user.visible_host.clone();

            // Build QUIT message to notify local users
            Some(Message {
                tags: None,
                prefix: Some(Prefix::Nickname(nick.clone(), user_str, host)),
                command: Command::QUIT(Some(quit_reason.clone())),
            })
        } else {
            None
        };

        // Remove user from channels first
        remove_user_from_channels(matrix, uid).await;

        // Remove user from user manager
        if let Some((_, _)) = matrix.user_manager.users.remove(uid) {
            // Clean up nicks map
            // We need to find the nick to remove
            let nick_to_remove: Option<String> = matrix
                .user_manager
                .nicks
                .iter()
                .find(|e| e.value() == uid)
                .map(|e| e.key().clone());

            if let Some(nick) = nick_to_remove {
                matrix.user_manager.nicks.remove(&nick);
            }

            // Remove sender if present
            matrix.user_manager.senders.remove(uid);
        }

        // Broadcast QUIT to local users
        if let Some(msg) = quit_msg {
            broadcast_to_local_users(matrix, msg).await;
        }
    }

    // 4. Remove affected servers from topology
    let sid_list: Vec<ServerId> = affected_sids.into_iter().collect();
    matrix.sync_manager.topology.remove_servers(&sid_list);

    // 5. Remove the dead link from direct links
    matrix.sync_manager.links.remove(dead_link_sid);

    info!(
        dead_sid = %dead_link_sid.as_str(),
        users_removed = affected_users.len(),
        servers_removed = sid_list.len(),
        "Netsplit cleanup complete"
    );
}

/// Remove a user from all channels they are in.
async fn remove_user_from_channels(matrix: &Matrix, uid: &str) {
    // Get list of channels the user is in
    let channels: Vec<String> = if let Some(user_arc) = matrix.user_manager.users.get(uid) {
        let user = user_arc.read().await;
        user.channels.iter().cloned().collect()
    } else {
        return;
    };

    // Send PART event to each channel actor
    for channel_name in channels {
        let channel_lower = slirc_proto::irc_to_lower(&channel_name);

        if let Some(channel_tx) = matrix.channel_manager.channels.get(&channel_lower) {
            use crate::state::actor::ChannelEvent;

            // Send a netsplit removal event
            // Using the PART event structure but could also be a dedicated event
            let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
            let event = ChannelEvent::NetsplitQuit {
                uid: uid.to_string(),
                reply_tx,
            };

            // Send and don't wait for response (async cleanup)
            let _ = channel_tx.send(event).await;
        }
    }
}

/// Broadcast a message to all local users.
async fn broadcast_to_local_users(matrix: &Matrix, msg: Message) {
    let msg_arc = Arc::new(msg);
    for entry in matrix.user_manager.senders.iter() {
        let sender = entry.value();
        let _ = sender.send(msg_arc.clone()).await;
    }
}

/// Calculate the netsplit quit reason from server names.
#[allow(dead_code)] // Used for netsplit QUIT message formatting
pub fn netsplit_reason(local_name: &str, remote_name: &str) -> String {
    format!("{} {}", local_name, remote_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netsplit_reason_format() {
        let reason = netsplit_reason("irc.local.net", "irc.remote.net");
        assert_eq!(reason, "irc.local.net irc.remote.net");
    }
}
