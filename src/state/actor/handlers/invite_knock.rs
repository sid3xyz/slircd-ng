//! INVITE and KNOCK event handling.
//!
//! Manages channel invitations and knock requests for +i channels.

use super::{ChannelActor, ChannelError, ChannelMode, InviteParams, Uid};
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_invite(
        &mut self,
        params: InviteParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        let InviteParams {
            sender_uid,
            sender_prefix,
            target_uid,
            target_nick,
            force,
            cap,
        } = params;

        let authorized = force || cap.is_some();

        // Check +V (no invites) - blocks all invitations to this channel
        // Only force (from capabilities/services) can bypass
        if !authorized && self.modes.contains(&ChannelMode::NoInvite) {
            let _ = reply_tx.send(Err(ChannelError::NoInviteActive));
            return;
        }

        if !authorized && self.modes.contains(&ChannelMode::InviteOnly) {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err(ChannelError::ChanOpPrivsNeeded));
                return;
            }
        }

        if self.members.contains_key(&target_uid) {
            let _ = reply_tx.send(Err(ChannelError::UserOnChannel(target_nick)));
            return;
        }

        self.add_invite(target_uid.clone());

        // Broadcast invite-notify
        let invite_msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::INVITE(target_nick, self.name.clone()),
        };

        // Broadcast invite-notify to all members with invite-notify capability
        if let Some(matrix) = self.matrix.upgrade() {
            let msg_arc = Arc::new(invite_msg);
            for (uid, _) in &self.members {
                if *uid == target_uid {
                    continue;
                }

                if let Some(caps) = self.user_caps.get(uid)
                    && caps.contains("invite-notify")
                {
                    matrix.user_manager.try_send_to_uid(uid, msg_arc.clone());
                }
            }
        }

        let _ = reply_tx.send(Ok(()));
    }

    pub(crate) async fn handle_knock(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        if self.modes.contains(&ChannelMode::NoKnock) {
            let _ = reply_tx.send(Err(ChannelError::CannotKnock));
            return;
        }

        if !self.modes.contains(&ChannelMode::InviteOnly) {
            let _ = reply_tx.send(Err(ChannelError::ChanOpen));
            return;
        }

        let nick = match &sender_prefix {
            Prefix::Nickname(n, _, _) => n.clone(),
            _ => "Unknown".to_string(),
        };

        if self.members.contains_key(&sender_uid) {
            let _ = reply_tx.send(Err(ChannelError::UserOnChannel(nick)));
            return;
        }

        let msg_text = format!("User {} is KNOCKing on {}", nick, self.name);
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::NOTICE(self.name.clone(), msg_text),
        };

        // Send knock notification to ops/halfops using multi-sender
        if let Some(matrix) = self.matrix.upgrade() {
            let msg_arc = Arc::new(msg);
            for (uid, modes) in &self.members {
                if modes.op || modes.halfop {
                    matrix.user_manager.try_send_to_uid(uid, msg_arc.clone());
                }
            }
        }

        let _ = reply_tx.send(Ok(()));
    }
}
