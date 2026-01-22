//! Netsplit handling for distributed IRC.
//!
//! When a server link drops, this module handles the cleanup:
//! - Identifies all servers that became unreachable
//! - Performs a "mass quit" for all affected users
//! - Updates the topology graph
//! - Notifies local clients of the splits

use crate::state::Matrix;
use slirc_proto::sync::clock::ServerId;
use slirc_proto::{BatchSubCommand, Command, Message, Prefix, Tag, generate_batch_ref};
use std::sync::Arc;
use tracing::{debug, info};

/// Handle a netsplit when a server link drops.
///
/// This function:
/// 1. Calculates all servers that became unreachable
/// 2. Removes all users from those servers
/// 3. Updates the topology graph
/// 4. Notifies local clients of the splits (using BATCH if capable)
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
    let quit_reason = netsplit_reason(local_name, remote_name);

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

    // Collection of QUIT messages for batch broadcast
    let mut quit_msgs = Vec::with_capacity(affected_users.len());

    // 3. Build QUIT messages and kill users (kill_user handles stats + cleanup)
    for uid in &affected_users {
        // Build QUIT message (before killing user)
        if let Some(user_arc) = matrix.user_manager.users.get(uid) {
            let user = user_arc.read().await;
            quit_msgs.push(Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    user.nick.clone(),
                    user.user.clone(),
                    user.visible_host.clone(),
                )),
                command: Command::QUIT(Some(quit_reason.clone())),
            });
        }

        // Remove from channels
        remove_user_from_channels(matrix, uid).await;

        // Kill user (handles stats, whowas, nicks, senders, observer)
        matrix
            .user_manager
            .kill_user(uid, &quit_reason, Some(dead_link_sid.clone()))
            .await;
    }

    // Broadcast QUITs to local users (using batch if possible)
    broadcast_netsplit_batch(matrix, remote_name, quit_msgs).await;

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
    let channels: Vec<String> = if let Some(user_arc) = matrix
        .user_manager
        .users
        .get(uid)
        .map(|u| u.value().clone())
    {
        let user = user_arc.read().await;
        user.channels.iter().cloned().collect()
    } else {
        return;
    };

    // Send PART event to each channel actor
    for channel_name in channels {
        let channel_lower = slirc_proto::irc_to_lower(&channel_name);

        let channel_tx = matrix
            .channel_manager
            .channels
            .get(&channel_lower)
            .map(|c| c.value().clone());

        if let Some(channel_tx) = channel_tx {
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

/// Broadcast a batch of netsplit QUITs to all local users.
///
/// Uses IRCv3 BATCH capability if supported by the client.
async fn broadcast_netsplit_batch(matrix: &Matrix, remote_server: &str, quit_msgs: Vec<Message>) {
    if quit_msgs.is_empty() {
        return;
    }

    let batch_ref = generate_batch_ref();

    // BATCH +ref netsplit <remote_server>
    let batch_start = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
        command: Command::BATCH(
            format!("+{}", batch_ref),
            Some(BatchSubCommand::NETSPLIT),
            Some(vec![remote_server.to_string()]),
        ),
    };

    // BATCH -ref
    let batch_end = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
        command: Command::BATCH(format!("-{}", batch_ref), None, None),
    };

    let start_arc = Arc::new(batch_start);
    let end_arc = Arc::new(batch_end);

    // Pre-calculate tagged messages for batch-capable clients
    let tagged_msgs: Vec<Arc<Message>> = quit_msgs
        .iter()
        .map(|msg| {
            let mut m = msg.clone();
            m.tags = Some(vec![Tag::new("batch", Some(batch_ref.clone()))]);
            Arc::new(m)
        })
        .collect();

    // Pre-calculate legacy messages
    let legacy_msgs: Vec<Arc<Message>> = quit_msgs.into_iter().map(Arc::new).collect();

    // Collect all sessions to iterate (avoids holding lock on senders map)
    // We need the sender and session_id
    let mut sessions = Vec::new();
    for entry in matrix.user_manager.senders.iter() {
        for session in entry.value() {
            sessions.push((session.tx.clone(), session.session_id));
        }
    }

    for (tx, session_id) in sessions {
        let caps = matrix
            .user_manager
            .get_session_caps(session_id)
            .unwrap_or_default();

        if caps.contains("batch") {
            // Send batch
            let _ = tx.try_send(start_arc.clone());
            for msg in &tagged_msgs {
                let _ = tx.try_send(msg.clone());
            }
            let _ = tx.try_send(end_arc.clone());
        } else {
            // Send individual messages
            for msg in &legacy_msgs {
                let _ = tx.try_send(msg.clone());
            }
        }
    }
}

/// Calculate the netsplit quit reason from server names.
fn netsplit_reason(local_name: &str, remote_name: &str) -> String {
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
