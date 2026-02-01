//! Message broadcasting to channel members.
//!
//! Handles fan-out of messages to all users in a channel,
//! including forwarding to remote peer servers.

use super::{ChannelActor, Uid};
use slirc_proto::{Command, Message, Prefix};
use std::collections::HashSet;
use std::sync::Arc;

use tracing::{trace, warn};

impl ChannelActor {
    pub(crate) async fn handle_broadcast(&mut self, message: Message, exclude: Option<Uid>) {
        let msg = Arc::new(message);

        // Broadcast to local users using UserManager's multi-sender infrastructure
        if let Some(matrix) = self.matrix.upgrade() {
            for uid in self.members.keys() {
                if exclude.as_ref() == Some(uid) {
                    continue;
                }
                matrix.user_manager.try_send_to_uid(uid, msg.clone());
            }
        }

        // Forward to peer servers
        self.forward_to_peers(&msg, exclude.as_ref()).await;
    }

    /// Forward a channel message to all connected peer servers.
    ///
    /// This is part of Innovation 2 (Distributed Server Linking).
    /// Messages are converted to S2S format with UID-based addressing.
    async fn forward_to_peers(&self, msg: &Message, exclude_uid: Option<&Uid>) {
        let Some(matrix) = self.matrix.upgrade() else {
            return;
        };

        // Skip if no peers connected
        if matrix.sync_manager.links.is_empty() {
            return;
        }

        // Convert to S2S format: :SourceUID PRIVMSG #channel :text
        let s2s_msg = match &msg.command {
            Command::PRIVMSG(target, text) => {
                // Get source UID from prefix or excluded UID
                let source_uid = msg
                    .prefix
                    .as_ref()
                    .map(|p| match p {
                        Prefix::Nickname(nick, _, _) => {
                            // Try to resolve nick to UID (take first UID for multiclient)
                            matrix
                                .user_manager
                                .get_first_uid(nick)
                                .unwrap_or_else(|| nick.clone())
                        }
                        Prefix::ServerName(name) => name.clone(),
                    })
                    .or_else(|| exclude_uid.cloned());

                let source = source_uid.unwrap_or_else(|| "unknown".to_string());

                Message {
                    tags: msg.tags.clone(),
                    prefix: Some(Prefix::new_from_str(&source)),
                    command: Command::PRIVMSG(target.clone(), text.clone()),
                }
            }
            Command::NOTICE(target, text) => {
                let source_uid = msg
                    .prefix
                    .as_ref()
                    .map(|p| match p {
                        Prefix::Nickname(nick, _, _) => matrix
                            .user_manager
                            .get_first_uid(nick)
                            .unwrap_or_else(|| nick.clone()),
                        Prefix::ServerName(name) => name.clone(),
                    })
                    .or_else(|| exclude_uid.cloned());

                let source = source_uid.unwrap_or_else(|| "unknown".to_string());

                Message {
                    tags: msg.tags.clone(),
                    prefix: Some(Prefix::new_from_str(&source)),
                    command: Command::NOTICE(target.clone(), text.clone()),
                }
            }
            _ => return, // Only forward PRIVMSG and NOTICE
        };

        // Get source server ID to avoid echo
        let source_sid = exclude_uid
            .filter(|uid| uid.len() >= 3)
            .map(|uid| &uid[0..3]);

        // Broadcast to all peers except source
        for entry in matrix.sync_manager.links.iter() {
            let peer_sid = entry.key();

            // Don't echo back to the source server
            if let Some(src_sid) = source_sid
                && peer_sid.as_str() == src_sid
            {
                trace!(peer = %peer_sid.as_str(), "Skipping source peer for channel message");
                continue;
            }

            let link = entry.value().clone();
            if let Err(e) = link.tx.send(Arc::new(s2s_msg.clone())).await {
                warn!(peer = %peer_sid.as_str(), error = %e, "Failed to forward channel message");
            } else {
                trace!(
                    peer = %peer_sid.as_str(),
                    channel = %self.name,
                    "Forwarded channel message to peer"
                );
            }
        }
    }

    pub(crate) async fn handle_broadcast_with_cap(
        &mut self,
        message: Message,
        exclude: Vec<Uid>,
        required_cap: Option<String>,
        fallback_msg: Option<Message>,
    ) {
        let msg = Arc::new(message);
        trace!(tags = ?msg.tags, "Broadcasting message with cap");
        let fallback = fallback_msg.map(Arc::new);

        // Track delivered UIDs to prevent duplicates across members and fan-out
        let mut delivered_uids: HashSet<Uid> = HashSet::new();

        // 1. Send to existing members
        // Use UserManager's multi-sender infrastructure for bouncer mode support
        if let Some(matrix) = self.matrix.upgrade() {
            for uid in self.members.keys() {
                if exclude.contains(uid) {
                    continue;
                }

                let should_send_main = if let Some(cap) = &required_cap {
                    if let Some(caps) = self.user_caps.get(uid) {
                        caps.contains(cap)
                    } else {
                        false
                    }
                } else {
                    true
                };

                let msg_to_send = if should_send_main {
                    msg.clone()
                } else if let Some(fb) = &fallback {
                    fb.clone()
                } else {
                    continue;
                };

                // Use try_send_to_uid which broadcasts to all sessions sharing this UID
                matrix.user_manager.try_send_to_uid(uid, msg_to_send);
                delivered_uids.insert(uid.clone());
            }
        }

        // 2. Multiclient Fan-out
        // Find other sessions of members that are not in the channel and send to them.
        if let Some(matrix) = self.matrix.upgrade() {
            let mut extra_targets: Vec<Uid> = Vec::new();

            // Identify potential targets
            for member_uid in self.members.keys() {
                if let Some(user_arc) = matrix.user_manager.users.get(member_uid) {
                    let account_opt = user_arc.read().await.account.clone();
                    if let Some(account) = account_opt {
                        // Inefficient: iterating all sessions. Ideally use get_client() -> sessions.
                        // But get_sessions is available public API.
                        let sessions = matrix.client_manager.get_sessions(&account);
                        for session in sessions {
                            if !delivered_uids.contains(&session.uid)
                                && !exclude.contains(&session.uid)
                                && !extra_targets.contains(&session.uid)
                            {
                                extra_targets.push(session.uid);
                            }
                        }
                    }
                }
            }

            // Send to extra targets
            for uid in extra_targets {
                if delivered_uids.contains(&uid) {
                    continue;
                }

                // Fetch caps for this user
                let caps_opt = if let Some(u) = matrix.user_manager.users.get(&uid) {
                    Some(u.read().await.caps.clone())
                } else {
                    None
                };

                let should_send_main = if let Some(cap) = &required_cap {
                    if let Some(caps) = &caps_opt {
                        caps.contains(cap)
                    } else {
                        false
                    }
                } else {
                    true
                };

                let msg_to_send = if should_send_main {
                    msg.clone()
                } else if let Some(fb) = &fallback {
                    fb.clone()
                } else {
                    continue;
                };

                matrix.user_manager.try_send_to_uid(&uid, msg_to_send);
                delivered_uids.insert(uid);
            }
        }
    }
}
