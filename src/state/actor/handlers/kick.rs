use super::{ChannelActor, Uid};
use slirc_proto::{Command, Message, Prefix};
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
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if !force {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err("ERR_CHANOPRIVSNEEDED".to_string()));
                return;
            }
        }

        if !self.members.contains_key(&target_uid) {
            let _ = reply_tx.send(Err("ERR_USERNOTINCHANNEL".to_string()));
            return;
        }

        let msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::KICK(self.name.clone(), target_nick, Some(reason)),
        };

        for sender in self.senders.values() {
            let _ = sender.send(msg.clone()).await;
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
