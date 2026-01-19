//! TOPIC event handling.
//!
//! Manages channel topic retrieval and modification with +t enforcement.

use super::{ChannelActor, ChannelError, ChannelMode, TopicParams};
use crate::state::Topic;
use slirc_proto::message::Tag;
use slirc_proto::sync::clock::HybridTimestamp;
use slirc_proto::{Command, Message};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_set_topic(
        &mut self,
        params: TopicParams,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        let TopicParams {
            sender_uid,
            sender_prefix,
            topic,
            msgid,
            timestamp,
            force,
            cap,
        } = params;

        let authorized = force || cap.is_some();

        if !authorized && self.modes.contains(&ChannelMode::TopicLock) {
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

        self.dirty = true;

        // Record timestamp for CRDT convergence
        self.topic_timestamp = Some(HybridTimestamp::now(&self.server_id));

        // Build TOPIC message with time and msgid tags for event-playback (Innovation 5)
        let tags = Some(vec![
            Tag(Cow::Borrowed("time"), Some(timestamp)),
            Tag(Cow::Borrowed("msgid"), Some(msgid.clone())),
        ]);

        let msg = Message {
            tags,
            prefix: Some(sender_prefix.clone()),
            command: Command::TOPIC(self.name.clone(), Some(topic.clone())),
        };

        // Store TOPIC event in history (EventPlayback)
        if let Some(matrix) = self.matrix.upgrade() {
            // Use provided msgid or new one
            let event_id = msgid.clone();
            let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
            let source = sender_prefix.to_string();

            let event = crate::history::types::HistoryItem::Event(crate::history::types::StoredEvent {
                id: event_id,
                nanotime: now,
                source,
                kind: crate::history::types::EventKind::Topic { 
                    old_topic: None, 
                    new_topic: topic 
                },
            });

            let history = matrix.service_manager.history.clone();
            let target = self.name.clone();
            tokio::spawn(async move {
                let _ = history.store_item(&target, event).await;
            });
        }

        for (uid, sender) in &self.senders {
            // Only send tags to clients that support message-tags
            let out_msg = if self
                .user_caps
                .get(uid)
                .is_some_and(|caps| caps.contains("message-tags"))
            {
                msg.clone()
            } else {
                // Strip tags for clients without message-tags capability
                Message {
                    tags: None,
                    prefix: msg.prefix.clone(),
                    command: msg.command.clone(),
                }
            };
            if let Err(err) = sender.try_send(Arc::new(out_msg)) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }

        self.notify_observer(None);
        let _ = reply_tx.send(Ok(()));
    }
}
