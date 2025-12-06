use super::{ChannelActor, ChannelMode, Uid};
use crate::state::Topic;
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_set_topic(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        topic: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), String>>,
    ) {
        if !force && self.modes.contains(&ChannelMode::TopicLock) {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err("ERR_CHANOPRIVSNEEDED".to_string()));
                return;
            }
        }

        self.topic = Some(Topic {
            text: topic.clone(),
            set_by: sender_prefix.to_string(),
            set_at: chrono::Utc::now().timestamp(),
        });

        let msg = Message {
            tags: None,
            prefix: Some(sender_prefix),
            command: Command::TOPIC(self.name.clone(), Some(topic)),
        };

        for sender in self.senders.values() {
            let _ = sender.send(msg.clone()).await;
        }

        let _ = reply_tx.send(Ok(()));
    }
}
