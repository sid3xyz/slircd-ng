use crate::state::Matrix;
use slirc_proto::Command;
use crate::state::actor::ChannelEvent;
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

    // 1. Burst Users (UID)
    // Iterate over all users. We only burst local users or users that we are responsible for?
    // Typically in a mesh, we burst all users we know about.
    // But for now, let's assume we burst all users in our user_manager.
    // Wait, if we have users from other servers, we should burst them too if we are acting as a hub.
    // But for the first hop, we definitely burst our local users.
    // Let's burst ALL users in the user_manager.

    for entry in state.user_manager.users.iter() {
        let user = entry.value().read().await;

        // UID nick hopcount timestamp username hostname uid modes realname
        // Note: hopcount should be incremented? Or is it 1 for local users?
        // For local users, hopcount is 1. For remote users, we increment.
        // But wait, `User` struct doesn't store hopcount directly?
        // We might need to infer it or store it.
        // For now, let's assume 1 for everyone as we are likely a leaf or single server.
        // TODO: Handle hopcounts correctly for multi-hop.

        let hopcount = "1".to_string(); // Placeholder
        let timestamp = "0".to_string(); // Placeholder: User struct needs creation TS?
        // User struct has `last_modified` (HybridTimestamp), but not creation TS?
        // Let's check User struct again.

        commands.push(Command::UID(
            user.nick.clone(),
            hopcount,
            timestamp, // TODO: Fix timestamp
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
        if let Err(e) = tx.send(ChannelEvent::GetInfo {
            requester_uid: None,
            reply_tx: info_tx,
        }).await {
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
        if let Err(e) = tx.send(ChannelEvent::GetMembers {
            reply_tx: members_tx,
        }).await {
            error!("Failed to request members for channel {}: {}", channel_name, e);
            continue;
        }

        let members = match members_rx.await {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to receive members for channel {}: {}", channel_name, e);
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
