use super::{ChannelActor, ChannelError, ChannelMode, Uid};
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::oneshot;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_invite(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        target_uid: Uid,
        target_nick: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        if !force && self.modes.contains(&ChannelMode::InviteOnly) {
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
            prefix: Some(sender_prefix.clone()),
            command: Command::INVITE(target_nick.clone(), self.name.clone()),
        };

        for (uid, _) in &self.members {
            if *uid == target_uid {
                continue;
            }

            if let Some(caps) = self.user_caps.get(uid)
                && caps.contains("invite-notify")
                && let Some(sender) = self.senders.get(uid)
            {
                let _ = sender.send(invite_msg.clone()).await;
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
            let _ = reply_tx.send(Err(ChannelError::UserOnChannel(nick.clone())));
            return;
        }

        let msg_text = format!("User {} is KNOCKing on {}", nick, self.name);
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::NOTICE(self.name.clone(), msg_text),
        };

        for (uid, modes) in &self.members {
            if (modes.op || modes.halfop)
                && let Some(sender) = self.senders.get(uid)
            {
                let _ = sender.send(msg.clone()).await;
            }
        }

        let _ = reply_tx.send(Ok(()));
    }
}
