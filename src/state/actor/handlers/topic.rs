use super::{ChannelActor, ChannelError, ChannelMode, Uid};
use crate::state::Topic;
use slirc_proto::message::Tag;
use slirc_proto::{Command, Message, Prefix};
use std::borrow::Cow;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_set_topic(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        topic: String,
        msgid: String,
        timestamp: String,
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

        // Build TOPIC message with time and msgid tags for event-playback (Innovation 5)
        let tags = Some(vec![
            Tag(Cow::Borrowed("time"), Some(timestamp)),
            Tag(Cow::Borrowed("msgid"), Some(msgid)),
        ]);

        let msg = Message {
            tags,
            prefix: Some(sender_prefix),
            command: Command::TOPIC(self.name.clone(), Some(topic)),
        };

        for (uid, sender) in &self.senders {
            // Only send tags to clients that support message-tags
            let out_msg = if self.user_caps.get(uid).is_some_and(|caps| caps.contains("message-tags")) {
                msg.clone()
            } else {
                // Strip tags for clients without message-tags capability
                Message {
                    tags: None,
                    prefix: msg.prefix.clone(),
                    command: msg.command.clone(),
                }
            };
            if let Err(err) = sender.try_send(out_msg) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }

        let _ = reply_tx.send(Ok(()));
    }
}
