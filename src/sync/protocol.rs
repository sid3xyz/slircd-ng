//! S2S Protocol Handling.
//!
//! Handles the serialization and deserialization of S2S commands,
//! and routing of messages between the network layer and the sync manager.

use crate::db::Shun;
use crate::state::actor::ChannelEvent;
use crate::state::observer::StateObserver;
use crate::state::{Matrix, User, UserModes};
use crate::sync::SyncManager;
use ipnet::IpNet;
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use slirc_proto::{Command, Message};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Parse an IP address or CIDR string into an IpNet.
fn parse_ip_or_cidr(ip: &str) -> Option<IpNet> {
    ip.parse().ok().or_else(|| {
        // Try parsing as single IP and convert to /32 or /128
        ip.parse::<IpAddr>().ok().map(|addr| match addr {
            IpAddr::V4(v4) => IpNet::V4(ipnet::Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
            IpAddr::V6(v6) => IpNet::V6(ipnet::Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
        })
    })
}

/// Handles incoming S2S commands after the handshake is complete.
pub struct IncomingCommandHandler {
    pub matrix: Arc<Matrix>,
}

impl IncomingCommandHandler {
    pub fn new(matrix: Arc<Matrix>) -> Self {
        Self { matrix }
    }

    /// Get reference to database via service manager.
    fn db(&self) -> &crate::db::Database {
        use crate::services::base::ServiceBase;
        self.matrix.service_manager.nickserv.db()
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
                    let _ = link.tx.send(Arc::new(Message::from(reply))).await;
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
            // ACCOUNT command for account status propagation (Innovation 2)
            Command::ACCOUNT(account) => {
                // Prefix contains the UID of the user whose account changed
                if let Some(uid) = prefix.map(|p| p.to_string()) {
                    self.handle_account(&uid, &account, source_sid).await;
                }
            }
            // PRIVMSG to service UIDs (NickServ/ChanServ)
            Command::PRIVMSG(target, text) => {
                if let Some(sender_uid) = prefix.map(|p| p.to_string()) {
                    self.handle_service_privmsg(&sender_uid, &target, &text)
                        .await;
                }
            }
            // Global bans received from peers via Raw commands
            Command::Raw(command, params) => {
                let setter = prefix
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| source_sid.as_str().to_string());
                self.handle_raw_command(&command, &params, setter).await;
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
                    .send(Arc::new(Message::from(Command::ERROR(format!(
                        "Loop detected: {} ({})",
                        name, sid
                    )))))
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

    /// Handle ACCOUNT command from peer - user's account status changed.
    async fn handle_account(&self, uid: &str, account: &str, source_sid: &ServerId) {
        // account = "*" means logout, otherwise it's the account name
        let account_opt = if account == "*" {
            None
        } else {
            Some(account.to_string())
        };

        info!(
            uid = %uid,
            account = %account,
            source = %source_sid.as_str(),
            "Received ACCOUNT update from peer"
        );

        // Update the user's account field if they exist locally
        if let Some(user_arc) = self.matrix.user_manager.users.get(uid) {
            let mut user = user_arc.write().await;
            user.account = account_opt.clone();
            // Update +r mode based on account status
            user.modes.registered = account_opt.is_some();
            debug!(uid = %uid, account = ?user.account, "Updated remote user account");
        } else {
            // User not found - may be on a different peer, or haven't received UID yet
            debug!(uid = %uid, "ACCOUNT for unknown UID (normal for remote users)");
        }

        // Propagate to other peers (flood)
        self.matrix.sync_manager.on_account_change(uid, account_opt.as_deref(), Some(source_sid.clone()));
    }

    /// Handle raw commands - primarily for global bans.
    async fn handle_raw_command(&self, command: &str, params: &[String], setter: String) {
        match command.to_uppercase().as_str() {
            "GLINE" => {
                // GLINE <mask> :<reason>
                if let Some(mask) = params.first() {
                    let reason = params
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "No reason".to_string());
                    self.apply_gline(mask, &reason, &setter).await;
                }
            }
            "UNGLINE" => {
                // UNGLINE <mask>
                if let Some(mask) = params.first() {
                    self.remove_gline(mask, &setter).await;
                }
            }
            "ZLINE" => {
                // ZLINE <ip> :<reason>
                if let Some(ip) = params.first() {
                    let reason = params
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "No reason".to_string());
                    self.apply_zline(ip, &reason, &setter).await;
                }
            }
            "UNZLINE" => {
                // UNZLINE <ip>
                if let Some(ip) = params.first() {
                    self.remove_zline(ip, &setter).await;
                }
            }
            "RLINE" => {
                // RLINE <regex> :<reason>
                if let Some(regex) = params.first() {
                    let reason = params
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "No reason".to_string());
                    self.apply_rline(regex, &reason, &setter).await;
                }
            }
            "UNRLINE" => {
                // UNRLINE <regex>
                if let Some(regex) = params.first() {
                    self.remove_rline(regex, &setter).await;
                }
            }
            "SHUN" => {
                // SHUN <mask> :<reason>
                if let Some(mask) = params.first() {
                    let reason = params
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "Shunned".to_string());
                    self.apply_shun(mask, &reason, &setter).await;
                }
            }
            "UNSHUN" => {
                // UNSHUN <mask>
                if let Some(mask) = params.first() {
                    self.remove_shun(mask, &setter).await;
                }
            }
            _ => {
                debug!("Ignoring unknown S2S raw command: {}", command);
            }
        }
    }

    // --- Global Ban Application Handlers ---

    async fn apply_gline(&self, mask: &str, reason: &str, setter: &str) {
        info!(
            mask = %mask,
            reason = %reason,
            setter = %setter,
            "Applying remote GLINE"
        );

        // Add to in-memory cache (expires_at = None for permanent)
        self.matrix.security_manager.ban_cache.add_gline(
            mask.to_string(),
            reason.to_string(),
            None,
        );

        // Persist to database
        if let Err(e) = self
            .db()
            .bans()
            .add_gline(mask, Some(reason), setter, None)
            .await
        {
            error!(error = %e, "Failed to persist remote GLINE to database");
        }
    }

    async fn remove_gline(&self, mask: &str, setter: &str) {
        info!(
            mask = %mask,
            setter = %setter,
            "Removing remote GLINE"
        );

        self.matrix.security_manager.ban_cache.remove_gline(mask);

        if let Err(e) = self.db().bans().remove_gline(mask).await {
            error!(error = %e, "Failed to remove remote GLINE from database");
        }
    }

    async fn apply_zline(&self, ip: &str, reason: &str, setter: &str) {
        info!(
            ip = %ip,
            reason = %reason,
            setter = %setter,
            "Applying remote ZLINE"
        );

        if let Some(net) = parse_ip_or_cidr(ip)
            && let Ok(mut deny_list) = self.matrix.security_manager.ip_deny_list.write()
            && let Err(e) = deny_list.add_ban(net, reason.to_string(), None, setter.to_string())
        {
            error!(error = %e, "Failed to add remote Z-line to IP deny list");
        }

        // Persist to database
        if let Err(e) = self
            .db()
            .bans()
            .add_zline(ip, Some(reason), setter, None)
            .await
        {
            error!(error = %e, "Failed to persist remote ZLINE to database");
        }
    }

    async fn remove_zline(&self, ip: &str, setter: &str) {
        info!(
            ip = %ip,
            setter = %setter,
            "Removing remote ZLINE"
        );

        if let Some(net) = parse_ip_or_cidr(ip)
            && let Ok(mut deny_list) = self.matrix.security_manager.ip_deny_list.write()
        {
            let _ = deny_list.remove_ban(net);
        }

        if let Err(e) = self.db().bans().remove_zline(ip).await {
            error!(error = %e, "Failed to remove remote ZLINE from database");
        }
    }

    async fn apply_rline(&self, regex: &str, reason: &str, setter: &str) {
        info!(
            regex = %regex,
            reason = %reason,
            setter = %setter,
            "Applying remote RLINE"
        );

        // R-lines don't have in-memory cache (checked via DB at connection time)
        // Just persist to database
        if let Err(e) = self
            .db()
            .bans()
            .add_rline(regex, Some(reason), setter, None)
            .await
        {
            error!(error = %e, "Failed to persist remote RLINE to database");
        }
    }

    async fn remove_rline(&self, regex: &str, setter: &str) {
        info!(
            regex = %regex,
            setter = %setter,
            "Removing remote RLINE"
        );

        if let Err(e) = self.db().bans().remove_rline(regex).await {
            error!(error = %e, "Failed to remove remote RLINE from database");
        }
    }

    async fn apply_shun(&self, mask: &str, reason: &str, setter: &str) {
        info!(
            mask = %mask,
            reason = %reason,
            setter = %setter,
            "Applying remote SHUN"
        );

        let now = chrono::Utc::now().timestamp();
        self.matrix.security_manager.shuns.insert(
            mask.to_string(),
            Shun {
                mask: mask.to_string(),
                reason: Some(reason.to_string()),
                set_by: setter.to_string(),
                set_at: now,
                expires_at: None,
            },
        );

        if let Err(e) = self
            .db()
            .bans()
            .add_shun(mask, Some(reason), setter, None)
            .await
        {
            error!(error = %e, "Failed to persist remote SHUN to database");
        }
    }

    async fn remove_shun(&self, mask: &str, setter: &str) {
        info!(
            mask = %mask,
            setter = %setter,
            "Removing remote SHUN"
        );

        self.matrix.security_manager.shuns.remove(mask);

        if let Err(e) = self.db().bans().remove_shun(mask).await {
            error!(error = %e, "Failed to remove remote SHUN from database");
        }
    }

    /// Handle PRIVMSG from remote users to service UIDs.
    ///
    /// When a remote user sends a PRIVMSG to NickServ or ChanServ UID,
    /// we route it to the appropriate service handler.
    async fn handle_service_privmsg(&self, sender_uid: &str, target: &str, text: &str) {
        // Check if target is one of our service UIDs
        if !self.matrix.service_manager.is_service_uid(target) {
            // Not a service UID - ignore (could be a channel or user)
            return;
        }

        // Get sender's nick for service replies
        let sender_nick = if let Some(user_arc) = self.matrix.user_manager.users.get(sender_uid) {
            user_arc.read().await.nick.clone()
        } else {
            warn!(uid = %sender_uid, "Unknown sender UID for service PRIVMSG");
            return;
        };

        debug!(
            sender = %sender_nick,
            target = %target,
            text = %text,
            "Routing remote PRIVMSG to service"
        );

        // Route to the appropriate service
        let effects = if target == self.matrix.service_manager.nickserv_uid {
            self.matrix
                .service_manager
                .nickserv
                .handle_command(&self.matrix, sender_uid, &sender_nick, text)
                .await
        } else if target == self.matrix.service_manager.chanserv_uid {
            self.matrix
                .service_manager
                .chanserv
                .handle_command(&self.matrix, sender_uid, &sender_nick, text)
                .await
        } else {
            // Should not happen given is_service_uid check
            return;
        };

        // Apply effects (particularly Reply effects to send responses back)
        crate::services::apply_effects_no_sender(&self.matrix, &sender_nick, effects).await;
    }
}
