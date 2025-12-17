//! KICK event handling.
//!
//! Removes users from channels with operator privilege checking.

use super::{ChannelActor, ChannelError, ChannelMode, Uid};
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_kick(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        reason: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        // Check +Q (no kicks) - even ops cannot kick when this is set
        // Only force (from capabilities/services) can bypass
        if !force && self.modes.contains(&ChannelMode::NoKicks) {
            let _ = reply_tx.send(Err(ChannelError::NoKicksActive));
            return;
        }

        if !force {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err(ChannelError::ChanOpPrivsNeeded));
                return;
            }
        }

        if !self.members.contains_key(&target_uid) {
            let _ = reply_tx.send(Err(ChannelError::UserNotInChannel(target_nick)));
            return;
        }

        let msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::KICK(self.name.clone(), target_nick, Some(reason)),
        };

        for (uid, sender) in &self.senders {
            if let Err(err) = sender.try_send(msg.clone()) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }

        self.members.remove(&target_uid);
        self.senders.remove(&target_uid);
        self.user_caps.remove(&target_uid);
        self.user_nicks.remove(&target_uid);
        self.kicked_users
            .insert(target_uid, std::time::Instant::now());

        // Update channel member count metric (Innovation 3)
        crate::metrics::set_channel_members(&self.name, self.members.len() as i64);

        let _ = reply_tx.send(Ok(()));
    }
}
