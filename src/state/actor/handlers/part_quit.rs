//! PART and QUIT event handling.
//!
//! Removes users from channels and broadcasts departure messages.

use super::{ChannelActor, ChannelError, Uid};
use slirc_proto::{Command, Message, Prefix};
use tokio::sync::oneshot;

impl ChannelActor {
    pub(crate) async fn handle_part(
        &mut self,
        uid: Uid,
        reason: Option<String>,
        prefix: Prefix,
        reply_tx: oneshot::Sender<Result<usize, ChannelError>>,
    ) {
        if !self.members.contains_key(&uid) {
            let _ = reply_tx.send(Err(ChannelError::NotOnChannel));
            return;
        }

        // Determine visibility for Auditorium mode (+u)
        let mut exclude = Vec::new();
        let is_auditorium = self
            .modes
            .contains(&crate::state::actor::ChannelMode::Auditorium);

        // Check if parter is privileged
        let parter_privileged = self
            .members
            .get(&uid)
            .map(|m| m.voice || m.halfop || m.op || m.admin || m.owner)
            .unwrap_or(false);

        if is_auditorium && !parter_privileged {
            // If +u and parter is not privileged, only privileged members see the PART.
            // So we exclude all non-privileged members, EXCEPT the parter themselves (so they see their own PART).
            for (member_uid, modes) in &self.members {
                if !modes.voice
                    && !modes.halfop
                    && !modes.op
                    && !modes.admin
                    && !modes.owner
                    && member_uid != &uid
                {
                    exclude.push(member_uid.clone());
                }
            }
        }

        // Broadcast PART
        let part_msg = Message {
            tags: None,
            prefix: Some(prefix),
            command: Command::PART(self.name.clone(), reason),
        };
        self.handle_broadcast_with_cap(part_msg, exclude, None, None)
            .await;

        // Remove member
        self.members.remove(&uid);
        self.senders.remove(&uid);
        self.user_caps.remove(&uid);
        self.user_nicks.remove(&uid);

        // Update channel member count metric (Innovation 3)
        crate::metrics::set_channel_members(&self.name, self.members.len() as i64);

        self.notify_observer(None);
        let _ = reply_tx.send(Ok(self.members.len()));

        self.cleanup_if_empty();
    }

    pub(crate) async fn handle_quit(
        &mut self,
        uid: Uid,
        quit_msg: Message,
        reply_tx: Option<oneshot::Sender<usize>>,
    ) {
        if self.members.contains_key(&uid) {
            // Determine visibility for Auditorium mode (+u)
            let mut exclude = Vec::new();
            let is_auditorium = self
                .modes
                .contains(&crate::state::actor::ChannelMode::Auditorium);

            // Check if quitter is privileged
            let quitter_privileged = self
                .members
                .get(&uid)
                .map(|m| m.voice || m.halfop || m.op || m.admin || m.owner)
                .unwrap_or(false);

            if is_auditorium && !quitter_privileged {
                // If +u and quitter is not privileged, only privileged members see the QUIT.
                // So we exclude all non-privileged members.
                for (member_uid, modes) in &self.members {
                    if !modes.voice && !modes.halfop && !modes.op && !modes.admin && !modes.owner {
                        exclude.push(member_uid.clone());
                    }
                }
            }

            self.handle_broadcast_with_cap(quit_msg, exclude, None, None)
                .await;
            self.members.remove(&uid);
            self.senders.remove(&uid);
            self.user_caps.remove(&uid);
            self.user_nicks.remove(&uid);

            // Update channel member count metric (Innovation 3)
            crate::metrics::set_channel_members(&self.name, self.members.len() as i64);
            self.notify_observer(None);
        }
        if let Some(tx) = reply_tx {
            let _ = tx.send(self.members.len());
        }

        self.cleanup_if_empty();
    }
}
