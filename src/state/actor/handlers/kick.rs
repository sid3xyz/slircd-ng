//! KICK event handling.
//!
//! Removes users from channels with operator privilege checking.

use super::{ChannelActor, ChannelError, ChannelMode, KickParams};
use slirc_proto::{Command, Message};
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_kick(
        &mut self,
        params: KickParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        let KickParams {
            sender_uid,
            sender_prefix,
            target_uid,
            target_nick,
            reason,
            force,
            cap,
        } = params;

        // Authorization check:
        // 1. Force (internal/service override)
        // 2. Capability token (proof of authorization)
        let authorized = force || cap.is_some();

        // Check +Q (no kicks) - even ops cannot kick when this is set
        // Only force/cap can bypass (if cap implies override, which it currently does for all ops)
        if !authorized && self.modes.contains(&ChannelMode::NoKicks) {
            let _ = reply_tx.send(Err(ChannelError::NoKicksActive));
            return;
        }

        if !authorized {
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

        let msg = Arc::new(Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::KICK(self.name.clone(), target_nick, Some(reason)),
        });

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

        self.notify_observer(None);
        let _ = reply_tx.send(Ok(()));
    }
}
