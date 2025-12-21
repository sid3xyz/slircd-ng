//! S2S Protocol Handling.
//!
//! Handles the serialization and deserialization of S2S commands,
//! and routing of messages between the network layer and the sync manager.

use crate::state::actor::ChannelEvent;
use crate::state::{Matrix, User, UserModes};
use crate::sync::SyncManager;
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use slirc_proto::{Command, Message};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Handles incoming S2S commands after the handshake is complete.
pub struct IncomingCommandHandler {
    pub matrix: Arc<Matrix>,
}

impl IncomingCommandHandler {
    pub fn new(matrix: Arc<Matrix>) -> Self {
        Self { matrix }
    }

    pub async fn handle_message(
        &self,
        message: Message,
        manager: &SyncManager,
        source_sid: &ServerId,
    ) -> Result<(), String> {
        let prefix = message.prefix.clone();
        match message.command {
            Command::PING(origin, _target) => {
                // Reply with PONG <local_sid> <origin>
                let reply = Command::PONG(manager.local_id.as_str().to_string(), Some(origin));
                if let Some(link) = manager.links.get(source_sid) {
                    let _ = link.tx.send(Message::from(reply)).await;
                }
            }
            Command::PONG(_origin, _target) => {
                if let Some(mut link) = manager.links.get_mut(source_sid) {
                    link.last_pong = Instant::now();
                }
            }
            Command::ERROR(msg) => {
                error!("Received ERROR from {}: {}", source_sid.as_str(), msg);
                return Err(format!("Remote error: {}", msg));
            }
            Command::SID(name, hopcount, sid, desc) => {
                self.handle_sid(name, hopcount, sid, desc, manager, source_sid)
                    .await?;
            }
            Command::UID(nick, hopcount, ts, user, host, uid, modes, realname) => {
                self.handle_uid(nick, hopcount, ts, user, host, uid, modes, realname)
                    .await;
            }
            Command::SJOIN(ts, channel, modes, args, users) => {
                self.handle_sjoin(ts, channel, modes, args, users).await;
            }
            Command::TMODE(ts, channel, modes, args) => {
                let setter = prefix
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| source_sid.as_str().to_string());
                self.handle_tmode(ts, channel, modes, args, setter).await;
            }
            Command::TOPIC(channel, topic_opt) => {
                let setter = prefix
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| source_sid.as_str().to_string());
                self.handle_topic(channel, topic_opt, setter).await;
            }
            Command::KICK(channel, target, reason) => {
                let sender = prefix
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| source_sid.as_str().to_string());
                self.handle_kick(channel, target, reason.unwrap_or_default(), sender)
                    .await;
            }
            Command::KILL(target, comment) => {
                let sender = prefix
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| source_sid.as_str().to_string());
                self.handle_kill(target, comment, sender).await;
            }
            _ => {
                // Ignore other commands for now
            }
        }
        Ok(())
    }

    async fn handle_sid(
        &self,
        name: String,
        _hopcount: String,
        sid: String,
        desc: String,
        manager: &SyncManager,
        source_sid: &ServerId,
    ) -> Result<(), String> {
        info!("Received SID: {} ({}) - {}", name, sid, desc);

        let sid_obj = ServerId::new(sid.clone());
        if manager.topology.servers.contains_key(&sid_obj) {
            error!("Loop detected! Server {} ({}) already exists", name, sid);
            if let Some(link) = manager.links.get(source_sid) {
                let _ = link
                    .tx
                    .send(Message::from(Command::ERROR(format!(
                        "Loop detected: {} ({})",
                        name, sid
                    ))))
                    .await;
            }
            return Err("Loop detected".to_string());
        }

        // TODO(Phase2): Add introduced server to TopologyGraph with correct hopcount and via.
        Ok(())
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
                '+' => {}
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

        // TODO(Phase2): Distinguish local vs remote users with add_remote_user method.
        self.matrix.user_manager.add_local_user(user).await;
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
        let tx = self
            .matrix
            .channel_manager
            .get_or_create_actor(channel_name.clone(), Arc::downgrade(&self.matrix))
            .await;

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
        if let Err(e) = tx
            .send(ChannelEvent::SJoin {
                ts,
                modes,
                mode_args,
                users,
            })
            .await
        {
            error!("Failed to send SJOIN to actor {}: {}", channel_name, e);
        }
    }

    async fn handle_tmode(
        &self,
        ts: u64,
        channel: String,
        modes: String,
        args: Vec<String>,
        setter: String,
    ) {
        if let Some(tx) = self
            .matrix
            .channel_manager
            .channels
            .get(&channel.to_lowercase())
        {
            let _ = tx
                .send(ChannelEvent::RemoteMode {
                    ts,
                    setter,
                    modes,
                    args,
                })
                .await;
        }
    }

    async fn handle_topic(&self, channel: String, topic_opt: Option<String>, setter: String) {
        if let Some(topic_str) = topic_opt {
            // Try to parse TS
            let (ts, topic) = if let Some((ts_str, rest)) = topic_str.split_once(' ') {
                if let Ok(ts) = ts_str.parse::<u64>() {
                    (ts, rest.to_string())
                } else {
                    (0, topic_str)
                }
            } else {
                (0, topic_str)
            };

            if let Some(tx) = self
                .matrix
                .channel_manager
                .channels
                .get(&channel.to_lowercase())
            {
                let _ = tx
                    .send(ChannelEvent::RemoteTopic { ts, setter, topic })
                    .await;
            }
        }
    }

    async fn handle_kick(&self, channel: String, target: String, reason: String, sender: String) {
        if let Some(tx) = self
            .matrix
            .channel_manager
            .channels
            .get(&channel.to_lowercase())
        {
            let _ = tx
                .send(ChannelEvent::RemoteKick {
                    sender,
                    target,
                    reason,
                })
                .await;
        }
    }

    async fn handle_kill(&self, target: String, comment: String, sender: String) {
        // Check if target is local
        // target can be UID or Nick.
        // S2S usually uses UID.
        if let Some(_user) = self.matrix.user_manager.users.get(&target) {
            // It's a UID
            info!(
                "Received KILL for local user {} from {}: {}",
                target, sender, comment
            );
            self.matrix
                .request_disconnect(&target, &format!("Killed by {}: {}", sender, comment));
        } else if let Some(uid) = self.matrix.user_manager.nicks.get(&target) {
            // It's a Nick
            let uid_str = uid.value().clone();
            info!(
                "Received KILL for local user {} ({}) from {}: {}",
                target, uid_str, sender, comment
            );
            self.matrix
                .request_disconnect(&uid_str, &format!("Killed by {}: {}", sender, comment));
        }
    }
}
