//! PART and QUIT event handling.
//!
//! Removes users from channels and broadcasts departure messages.

use super::{ChannelActor, ChannelError, Uid};
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_part(
        &mut self,
        uid: Uid,
        reason: Option<String>,
        prefix: Prefix,
        reply_tx: oneshot::Sender<Result<usize, ChannelError>>,
    ) {
        if !self.members.contains_key(&uid) {
            let _ = reply_tx.send(Err(ChannelError::NotOnChannel));
            return;
        }

        // Broadcast PART
        let part_msg = Message {
            tags: None,
            prefix: Some(prefix),
            command: Command::PART(self.name.clone(), reason),
        };
        self.handle_broadcast(part_msg, None).await;

        // Remove member
        self.members.remove(&uid);
        self.senders.remove(&uid);
        self.user_caps.remove(&uid);
        self.user_nicks.remove(&uid);

        // Update channel member count metric (Innovation 3)
        crate::metrics::set_channel_members(&self.name, self.members.len() as i64);

        let _ = reply_tx.send(Ok(self.members.len()));

        self.cleanup_if_empty();
    }

    pub(crate) async fn handle_quit(
        &mut self,
        uid: Uid,
        quit_msg: Message,
        reply_tx: Option<oneshot::Sender<usize>>,
    ) {
        if self.members.contains_key(&uid) {
            self.handle_broadcast(quit_msg, None).await;
            self.members.remove(&uid);
            self.senders.remove(&uid);
            self.user_caps.remove(&uid);
            self.user_nicks.remove(&uid);

            // Update channel member count metric (Innovation 3)
            crate::metrics::set_channel_members(&self.name, self.members.len() as i64);
        }
        if let Some(tx) = reply_tx {
            let _ = tx.send(self.members.len());
        }

        self.cleanup_if_empty();
    }
}
