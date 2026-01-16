//! Message broadcasting to channel members.
//!
//! Handles fan-out of messages to all users in a channel,
//! including forwarding to remote peer servers.

use super::{ChannelActor, Uid};
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tracing::{debug, warn};

impl ChannelActor {
    pub(crate) async fn handle_broadcast(&mut self, message: Message, exclude: Option<Uid>) {
        let msg = Arc::new(message);

        // Broadcast to local users
        for (uid, sender) in &self.senders {
            if exclude.as_ref() == Some(uid) {
                continue;
            }
            if let Err(err) = sender.try_send(msg.clone()) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
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
                debug!(peer = %peer_sid.as_str(), "Skipping source peer for channel message");
                continue;
            }

            let link = entry.value().clone();
            if let Err(e) = link.tx.send(Arc::new(s2s_msg.clone())).await {
                warn!(peer = %peer_sid.as_str(), error = %e, "Failed to forward channel message");
            } else {
                debug!(
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
        debug!(tags = ?msg.tags, "Broadcasting message with cap");
        let fallback = fallback_msg.map(Arc::new);

        for (uid, sender) in &self.senders {
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

            if should_send_main {
                if let Err(err) = sender.try_send(msg.clone()) {
                    match err {
                        TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                        TrySendError::Closed(_) => {}
                    }
                }
            } else if let Some(fb) = &fallback
                && let Err(err) = sender.try_send(fb.clone())
            {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }
    }
}
