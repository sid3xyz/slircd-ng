#![allow(clippy::collapsible_if)]
use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::{ServerState, User, UserModes};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;
use slirc_crdt::clock::ServerId;

/// Handler for the UID command (User ID).
///
/// UID introduces a new user to the network.
pub struct UidHandler;

#[async_trait]
impl ServerHandler for UidHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: UID <nick> <hopcount> <timestamp> <username> <hostname> <uid> <modes> <realname>

        let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let hopcount_str = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let timestamp_str = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
        let username = msg.arg(3).ok_or(HandlerError::NeedMoreParams)?;
        let hostname = msg.arg(4).ok_or(HandlerError::NeedMoreParams)?;
        let uid = msg.arg(5).ok_or(HandlerError::NeedMoreParams)?;
        let modes_str = msg.arg(6).ok_or(HandlerError::NeedMoreParams)?;
        let realname = msg.arg(7).ok_or(HandlerError::NeedMoreParams)?;

        let _hopcount = hopcount_str.parse::<u32>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid hopcount: {}", hopcount_str))
        })?;

        let timestamp = timestamp_str.parse::<i64>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid timestamp: {}", timestamp_str))
        })?;

        // Parse modes
        let mut modes = UserModes::default();
        for c in modes_str.chars() {
            match c {
                '+' => continue,
                'i' => modes.invisible = true,
                'w' => modes.wallops = true,
                'o' => modes.oper = true,
                'r' => modes.registered = true,
                'Z' => modes.secure = true,
                'R' => modes.registered_only = true,
                'T' => modes.no_ctcp = true,
                'B' => modes.bot = true,
                'S' => modes.service = true,
                _ => {}
            }
        }

        let user = User {
            uid: uid.to_string(),
            nick: nick.to_string(),
            user: username.to_string(),
            realname: realname.to_string(),
            host: hostname.to_string(),
            ip: "0.0.0.0".to_string(), // Remote user IP unknown
            visible_host: hostname.to_string(), // Assume visible host is same as host for now
            session_id: Uuid::new_v4(),
            channels: HashSet::new(),
            modes,
            account: None,
            away: None,
            caps: HashSet::new(),
            certfp: None,
            silence_list: HashSet::new(),
            accept_list: HashSet::new(),
            created_at: timestamp,
            last_modified: slirc_crdt::clock::HybridTimestamp::now(&ServerId::new(ctx.state.sid.clone())),
        };

        let nick_lower = slirc_proto::irc_to_lower(nick);

        // Check collision
        if let Some(existing_uid) = ctx.matrix.user_manager.nicks.get(&nick_lower) {
            if *existing_uid != uid {
                // Collision detected.
                // TS6 Rule: Compare timestamps.
                // If incoming is older (lower TS), it wins. Kill existing.
                // If incoming is newer (higher TS), it loses. Kill incoming.
                // If equal, kill both.

                let existing_ts = if let Some(u) = ctx.matrix.user_manager.users.get(&*existing_uid) {
                    u.read().await.created_at
                } else {
                    0 // Should not happen
                };

                if timestamp < existing_ts {
                    // Incoming wins
                    info!(nick = %nick, "Nick collision: Incoming UID {} wins (older)", uid);
                    ctx.matrix.user_manager.kill_user(&existing_uid, "Nick collision (older wins)").await;
                } else if timestamp > existing_ts {
                    // Incoming loses
                    info!(nick = %nick, "Nick collision: Incoming UID {} loses (newer)", uid);

                    // Send KILL to peer for the incoming user
                    let kill_msg = slirc_proto::Message {
                        tags: None,
                        prefix: Some(ctx.server_prefix()),
                        command: slirc_proto::Command::KILL(uid.to_string(), "Nick collision (newer loses)".to_string()),
                    };
                    if let Err(e) = ctx.sender.send(kill_msg).await {
                         tracing::error!("Failed to send KILL for collision: {}", e);
                    }

                    return Ok(());
                } else {
                    // Tie - kill both
                    info!(nick = %nick, "Nick collision: Tie. Killing both.");
                    ctx.matrix.user_manager.kill_user(&existing_uid, "Nick collision (tie)").await;

                    // Send KILL to peer for the incoming user
                    let kill_msg = slirc_proto::Message {
                        tags: None,
                        prefix: Some(ctx.server_prefix()),
                        command: slirc_proto::Command::KILL(uid.to_string(), "Nick collision (tie)".to_string()),
                    };
                    if let Err(e) = ctx.sender.send(kill_msg).await {
                         tracing::error!("Failed to send KILL for collision: {}", e);
                    }

                    return Ok(());
                }
            }
        }

        ctx.matrix.user_manager.nicks.insert(nick_lower, uid.to_string());
        ctx.matrix.user_manager.users.insert(uid.to_string(), Arc::new(RwLock::new(user)));

        info!(uid = %uid, nick = %nick, "Registered remote user via UID");

        Ok(())
    }
}
