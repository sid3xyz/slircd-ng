use super::{ChannelActor, Uid};
use slirc_proto::Message;
use std::sync::Arc;

impl ChannelActor {
    pub(crate) async fn handle_broadcast(&mut self, message: Message, exclude: Option<Uid>) {
        let msg = Arc::new(message);
        for (uid, sender) in &self.senders {
            if exclude.as_ref() == Some(uid) {
                continue;
            }
            let _ = sender.send((*msg).clone()).await;
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
                let _ = sender.send((*msg).clone()).await;
            } else if let Some(fb) = &fallback {
                let _ = sender.send((**fb).clone()).await;
            }
        }
    }
}
