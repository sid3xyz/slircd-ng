//! State Burst Generation for S2S Synchronization.
//!
//! When a new server link is established, both sides exchange a "burst"
//! containing their complete state. This module generates the burst commands:
//! - Global bans (G-lines, Z-lines, Shuns) - sent first
//! - `UID` for each user (including service pseudoclients)
//! - `SJOIN` for each channel (with members, modes, topic)
//!
//! The burst is sent after handshake completion and before operational messages.

use crate::state::Matrix;
use crate::state::actor::ChannelEvent;
use slirc_proto::Command;
use tokio::sync::oneshot;
use tracing::error;

/// Generates the burst of commands to synchronize state with a new peer.
///
/// This function iterates over the local state (users and channels) and generates
/// the corresponding UID and SJOIN commands.
///
/// # Arguments
///
/// * `state` - The global server state (Matrix).
/// * `local_sid` - The local server ID (used for hopcounts).
pub async fn generate_burst(state: &Matrix, _local_sid: &str) -> Vec<Command> {
    let mut commands = Vec::new();

    // 0. Burst Global Bans (before users/channels to prevent race conditions)
    // G-lines
    for (mask, reason, _expires) in state.security_manager.ban_cache.iter_glines() {
        commands.push(Command::GLINE(mask, Some(reason)));
    }

    // Shuns
    for entry in state.security_manager.shuns.iter() {
        let shun = entry.value();
        commands.push(Command::SHUN(shun.mask.clone(), shun.reason.clone()));
    }

    // Z-lines (IP bans from ip_deny_list)
    // Note: Use ok() to gracefully handle lock poisoning - if the lock is poisoned,
    // skip Z-line burst rather than crash. The peer will sync eventually.
    if let Ok(ip_deny) = state.security_manager.ip_deny_list.read() {
        for (ip_mask, meta) in ip_deny.iter() {
            if !meta.is_expired() {
                commands.push(Command::ZLINE(ip_mask.clone(), Some(meta.reason.clone())));
            }
        }
    } else {
        error!("ip_deny_list lock poisoned, skipping Z-line burst");
    }

    // 1. Burst Users (UID)
    // Iterate over all users. We only burst local users or users that we are responsible for?
    // Typically in a mesh, we burst all users we know about.
    // But for now, let's assume we burst all users in our user_manager.
    // Wait, if we have users from other servers, we should burst them too if we are acting as a hub.
    // But for the first hop, we definitely burst our local users.
    // Let's burst ALL users in the user_manager.

    // Collect user Arcs to release DashMap lock before awaiting
    let user_arcs: Vec<_> = state
        .user_manager
        .users
        .iter()
        .map(|e| e.value().clone())
        .collect();

    for user_arc in user_arcs {
        let user = user_arc.read().await;

        // UID nick hopcount timestamp username hostname uid modes realname
        // For local users, hopcount is 1. For remote users, increment on relay.
        // Phase 2 note: When multi-hop is implemented, store hopcount in User struct.
        let hopcount = "1".to_string();
        let timestamp = user.created_at.to_string();

        commands.push(Command::UID(
            user.nick.clone(),
            hopcount,
            timestamp,
            user.user.clone(),
            user.visible_host.clone(),
            user.uid.clone(),
            user.modes.as_mode_string(),
            user.realname.clone(),
        ));
    }

    // 2. Burst Channels (SJOIN)
    for entry in state.channel_manager.channels.iter() {
        let channel_name = entry.key();
        let tx = entry.value();

        // Get Channel Info (Modes, Topic, TS)
        let (info_tx, info_rx) = oneshot::channel();
        if let Err(e) = tx
            .send(ChannelEvent::GetInfo {
                requester_uid: None,
                reply_tx: info_tx,
            })
            .await
        {
            error!("Failed to request info for channel {}: {}", channel_name, e);
            continue;
        }

        let info = match info_rx.await {
            Ok(i) => i,
            Err(e) => {
                error!("Failed to receive info for channel {}: {}", channel_name, e);
                continue;
            }
        };

        // Get Members (UIDs and Prefixes)
        let (members_tx, members_rx) = oneshot::channel();
        if let Err(e) = tx
            .send(ChannelEvent::GetMembers {
                reply_tx: members_tx,
            })
            .await
        {
            error!(
                "Failed to request members for channel {}: {}",
                channel_name, e
            );
            continue;
        }

        let members = match members_rx.await {
            Ok(m) => m,
            Err(e) => {
                error!(
                    "Failed to receive members for channel {}: {}",
                    channel_name, e
                );
                continue;
            }
        };

        // Construct SJOIN
        // SJOIN ts channel modes [args...] :users

        // Convert modes to string and args
        // ChannelInfo has `modes: HashSet<ChannelMode>`.
        // We need to convert this to "+nt" and args.
        // This is tricky without a helper.
        // `ChannelActor` has `modes_to_string`.
        // But `ChannelInfo` just has the set.
        // Wait, `ChannelInfo` has `modes: HashSet<ChannelMode>`.
        // `ChannelMode` enum has variants like `Key(String, TS)`.
        // So we can extract args.

        let mode_string_full = crate::state::actor::modes_to_string(&info.modes);
        let mut parts = mode_string_full.split_whitespace();
        let mode_str = parts.next().unwrap_or("+").to_string();
        let mode_args: Vec<String> = parts.map(|s| s.to_string()).collect();

        // Convert members to (prefix, uid) list
        let mut user_list = Vec::new();
        for (uid, modes) in members {
            let prefixes = modes.all_prefix_chars();
            user_list.push((prefixes, uid));
        }

        commands.push(Command::SJOIN(
            info.created as u64,
            info.name,
            mode_str,
            mode_args,
            user_list,
        ));
    }

    commands
}
