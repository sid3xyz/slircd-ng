//! StateObserver implementation for SyncManager.
//!
//! Propagates local state changes to connected peer servers.
//! This is the real-time delta propagation component of Innovation 2.

use crate::state::observer::StateObserver;
use crate::sync::LinkState;
use slirc_crdt::channel::ChannelCrdt;
use slirc_crdt::clock::ServerId;
use slirc_crdt::user::UserCrdt;
use slirc_proto::{Command, Message};
use tracing::{debug, info, warn};

use super::SyncManager;

impl SyncManager {
    /// Broadcast a message to all connected peers except the source.
    ///
    /// This implements split-horizon: we never echo a message back to its origin.
    #[allow(dead_code)] // Will be used when full burst/sync is implemented
    async fn broadcast_to_peers(&self, msg: Message, source: Option<&ServerId>) {
        for entry in self.links.iter() {
            let peer_sid = entry.key();
            
            // Split-horizon: don't send back to source
            if let Some(src) = source
                && peer_sid == src
            {
                debug!(peer = %peer_sid.as_str(), "Skipping source peer (split-horizon)");
                continue;
            }
            
            let link: LinkState = entry.value().clone();
            if let Err(e) = link.tx.send(msg.clone()).await {
                warn!(peer = %peer_sid.as_str(), error = %e, "Failed to send to peer");
            } else {
                debug!(peer = %peer_sid.as_str(), cmd = ?msg.command, "Sent to peer");
            }
        }
    }

    /// Build an SJOIN command for a channel state.
    fn build_sjoin_command(&self, channel: &ChannelCrdt) -> Command {
        // SJOIN timestamp channel modes [args] :[@user1 +user2 ...]
        let ts = chrono::Utc::now().timestamp() as u64;
        
        // Collect modes
        let mut modes = String::new();
        let mut mode_args = Vec::new();
        
        if *channel.modes.no_external.value() { modes.push('n'); }
        if *channel.modes.topic_ops_only.value() { modes.push('t'); }
        if *channel.modes.moderated.value() { modes.push('m'); }
        if *channel.modes.invite_only.value() { modes.push('i'); }
        if *channel.modes.secret.value() { modes.push('s'); }
        if *channel.modes.private.value() { modes.push('p'); }
        if *channel.modes.registered_only.value() { modes.push('R'); }
        if *channel.modes.no_colors.value() { modes.push('c'); }
        if *channel.modes.no_ctcp.value() { modes.push('C'); }
        if *channel.modes.ssl_only.value() { modes.push('z'); }
        
        if let Some(key) = channel.key.value() {
            modes.push('k');
            mode_args.push(key.clone());
        }
        if let Some(limit) = channel.limit.value() {
            modes.push('l');
            mode_args.push(limit.to_string());
        }
        
        if modes.is_empty() {
            modes.push('+');
        } else {
            modes.insert(0, '+');
        }
        
        // Collect users with their modes
        let mut users = Vec::new();
        for uid in channel.members.iter() {
            if let Some(modes_crdt) = channel.members.get_modes(uid) {
                let mut prefix = String::new();
                if *modes_crdt.owner.value() { prefix.push('~'); }
                if *modes_crdt.admin.value() { prefix.push('&'); }
                if *modes_crdt.op.value() { prefix.push('@'); }
                if *modes_crdt.halfop.value() { prefix.push('%'); }
                if *modes_crdt.voice.value() { prefix.push('+'); }
                users.push((prefix, uid.clone()));
            }
        }
        
        Command::SJOIN(ts, channel.name.clone(), modes, mode_args, users)
    }

    /// Build a UID command for a user.
    fn build_uid_command(&self, user: &UserCrdt) -> Command {
        // UID nick hopcount ts user host uid modes :realname
        let ts = chrono::Utc::now().timestamp().to_string();
        let hopcount = "1".to_string();
        
        // Build mode string
        let mut modes = "+".to_string();
        if *user.modes.invisible.value() { modes.push('i'); }
        if *user.modes.oper.value() { modes.push('o'); }
        if *user.modes.registered.value() { modes.push('r'); }
        if *user.modes.wallops.value() { modes.push('w'); }
        if *user.modes.secure.value() { modes.push('Z'); }
        if *user.modes.bot.value() { modes.push('B'); }
        
        Command::UID(
            user.nick.value().clone(),
            hopcount,
            ts,
            user.user.value().clone(),
            user.host.value().clone(),
            user.uid.clone(),
            modes,
            user.realname.value().clone(),
        )
    }

    /// Build a QUIT command for a user.
    #[allow(dead_code)] // Will be used when quit propagation uses Command enum
    fn build_quit_command(&self, _uid: &str, reason: &str) -> Command {
        // QUIT is sent with UID prefix
        Command::Raw(
            "QUIT".to_string(),
            vec![format!(":{}", reason)],
        )
    }
}

impl StateObserver for SyncManager {
    fn on_user_update(&self, user: &UserCrdt, source: Option<ServerId>) {
        if source.is_some() {
            // This update came from a remote peer, don't re-broadcast
            debug!(uid = %user.uid, "Skipping user update (remote origin)");
            return;
        }

        info!(uid = %user.uid, nick = %user.nick.value(), "Broadcasting user update to peers");
        
        let msg = Message::from(self.build_uid_command(user));
        let links = self.links.clone();
        
        // Spawn async broadcast (we can't await in a sync trait method)
        tokio::spawn(async move {
            for entry in links.iter() {
                let link: LinkState = entry.value().clone();
                if let Err(e) = link.tx.send(msg.clone()).await {
                    warn!(peer = %entry.key().as_str(), error = %e, "Failed to send UID");
                }
            }
        });
    }

    fn on_user_quit(&self, uid: &str, reason: &str, source: Option<ServerId>) {
        if source.is_some() {
            debug!(uid = %uid, "Skipping quit broadcast (remote origin)");
            return;
        }

        info!(uid = %uid, reason = %reason, "Broadcasting QUIT to peers");
        
        // Build QUIT message with UID prefix
        let quit_msg = Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::new_from_str(uid)),
            command: Command::QUIT(Some(reason.to_string())),
        };
        
        let links = self.links.clone();
        
        tokio::spawn(async move {
            for entry in links.iter() {
                let link: LinkState = entry.value().clone();
                if let Err(e) = link.tx.send(quit_msg.clone()).await {
                    warn!(peer = %entry.key().as_str(), error = %e, "Failed to send QUIT");
                }
            }
        });
    }

    fn on_channel_update(&self, channel: &ChannelCrdt, source: Option<ServerId>) {
        if source.is_some() {
            debug!(channel = %channel.name, "Skipping channel update (remote origin)");
            return;
        }

        info!(channel = %channel.name, members = channel.members.len(), "Broadcasting channel update to peers");
        
        let msg = Message::from(self.build_sjoin_command(channel));
        let links = self.links.clone();
        
        tokio::spawn(async move {
            for entry in links.iter() {
                let link: LinkState = entry.value().clone();
                if let Err(e) = link.tx.send(msg.clone()).await {
                    warn!(peer = %entry.key().as_str(), error = %e, "Failed to send SJOIN");
                }
            }
        });
    }

    fn on_channel_destroy(&self, name: &str, source: Option<ServerId>) {
        if source.is_some() {
            debug!(channel = %name, "Skipping channel destroy (remote origin)");
            return;
        }

        // For channel destruction, we don't typically send a specific message.
        // The channel becomes empty and peers will clean up on their own.
        // However, we could send a "MODE #channel +P" removal or similar.
        info!(channel = %name, "Channel destroyed (no propagation needed)");
    }
}
