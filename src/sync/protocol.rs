//! S2S Protocol Handling.
//!
//! Handles the serialization and deserialization of S2S commands,
//! and routing of messages between the network layer and the sync manager.

use slirc_proto::Command;
use crate::state::{Matrix, User, UserModes};
use crate::state::actor::ChannelEvent;
use std::sync::Arc;
use tracing::{info, warn, error};
use uuid::Uuid;
use slirc_crdt::clock::HybridTimestamp;

/// Handles incoming S2S commands after the handshake is complete.
pub struct IncomingCommandHandler {
    pub matrix: Arc<Matrix>,
}

impl IncomingCommandHandler {
    pub fn new(matrix: Arc<Matrix>) -> Self {
        Self { matrix }
    }

    pub async fn handle_command(&self, command: Command) -> Result<(), String> {
        match command {
            Command::SID(name, hopcount, sid, desc) => {
                self.handle_sid(name, hopcount, sid, desc).await;
            }
            Command::UID(nick, hopcount, ts, user, host, uid, modes, realname) => {
                self.handle_uid(nick, hopcount, ts, user, host, uid, modes, realname).await;
            }
            Command::SJOIN(ts, channel, modes, args, users) => {
                self.handle_sjoin(ts, channel, modes, args, users).await;
            }
            _ => {
                // Ignore other commands for now
            }
        }
        Ok(())
    }

    async fn handle_sid(&self, name: String, _hopcount: String, sid: String, desc: String) {
        info!("Received SID: {} ({}) - {}", name, sid, desc);
        // TODO: Add to TopologyGraph
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_uid(
        &self,
        nick: String,
        _hopcount: String,
        _ts: String,
        username: String,
        host: String,
        uid: String,
        modes_str: String,
        realname: String,
    ) {
        info!("Received UID: {} ({})", nick, uid);

        // Parse modes
        let mut modes = UserModes::default();
        for c in modes_str.chars() {
            match c {
                '+' => {},
                'i' => modes.invisible = true,
                'w' => modes.wallops = true,
                'o' => modes.oper = true,
                'r' => modes.registered = true,
                'Z' => modes.secure = true,
                'R' => modes.registered_only = true,
                'T' => modes.no_ctcp = true,
                'B' => modes.bot = true,
                _ => {}
            }
        }

        let user = User {
            uid: uid.clone(),
            nick: nick.clone(),
            user: username,
            realname,
            host: host.clone(),
            ip: "0.0.0.0".to_string(), // Remote users don't have IP exposed usually
            visible_host: host,
            session_id: Uuid::new_v4(),
            channels: std::collections::HashSet::new(),
            modes,
            account: None,
            away: None,
            caps: std::collections::HashSet::new(),
            certfp: None,
            silence_list: std::collections::HashSet::new(),
            accept_list: std::collections::HashSet::new(),
            last_modified: HybridTimestamp::now(&self.matrix.server_id), // Should use TS from command
        };

        self.matrix.user_manager.add_local_user(user).await; // TODO: add_remote_user?
        // For now add_local_user just inserts into DashMap, which is fine.
    }

    async fn handle_sjoin(
        &self,
        ts: u64,
        channel_name: String,
        modes: String,
        mode_args: Vec<String>,
        users: Vec<(String, String)>,
    ) {
        info!("Received SJOIN for {}", channel_name);

        // 1. Get or Create Channel Actor
        let tx = self.matrix.channel_manager.get_or_create_actor(channel_name.clone(), Arc::downgrade(&self.matrix)).await;

        // 2. Update Users' channel lists
        for (_, uid) in &users {
            if let Some(user_arc) = self.matrix.user_manager.users.get(uid) {
                let mut user = user_arc.write().await;
                user.channels.insert(channel_name.clone());
            } else {
                warn!("SJOIN references unknown UID: {}", uid);
            }
        }

        // 3. Send SJoin event to Actor
        if let Err(e) = tx.send(ChannelEvent::SJoin {
            ts,
            modes,
            mode_args,
            users,
        }).await {
            error!("Failed to send SJOIN to actor {}: {}", channel_name, e);
        }
    }
}
