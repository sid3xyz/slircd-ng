use super::{ChannelActor, Uid};
use slirc_proto::Message;
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;

impl ChannelActor {
    pub(crate) async fn handle_broadcast(&mut self, message: Message, exclude: Option<Uid>) {
        let msg = Arc::new(message);
        for (uid, sender) in &self.senders {
            if exclude.as_ref() == Some(uid) {
                continue;
            }
            if let Err(err) = sender.try_send((*msg).clone()) {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }
    }

    pub(crate) async fn handle_broadcast_with_cap(
        &mut self,
        message: Message,
        exclude: Vec<Uid>,
        required_cap: Option<String>,
        fallback_msg: Option<Message>,
    ) {
        let msg = Arc::new(message);
        let fallback = fallback_msg.map(Arc::new);

        for (uid, sender) in &self.senders {
            if exclude.contains(uid) {
                continue;
            }

            let should_send_main = if let Some(cap) = &required_cap {
                if let Some(caps) = self.user_caps.get(uid) {
                    caps.contains(cap)
                } else {
                    false
                }
            } else {
                true
            };

            if should_send_main {
                if let Err(err) = sender.try_send((*msg).clone()) {
                    match err {
                        TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                        TrySendError::Closed(_) => {}
                    }
                }
            } else if let Some(fb) = &fallback
                && let Err(err) = sender.try_send((**fb).clone())
            {
                match err {
                    TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                    TrySendError::Closed(_) => {}
                }
            }
        }
    }
}
