use super::{ChannelActor, ChannelError, ChannelMode, Uid};
use crate::state::Topic;
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_set_topic(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        topic: String,
        force: bool,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        if !force && self.modes.contains(&ChannelMode::TopicLock) {
            let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
            if !sender_modes.op && !sender_modes.halfop {
                let _ = reply_tx.send(Err(ChannelError::ChanOpPrivsNeeded));
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

        for (uid, sender) in &self.senders {
            if let Err(err) = sender.try_send(msg.clone()) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }

        let _ = reply_tx.send(Ok(()));
    }
}
