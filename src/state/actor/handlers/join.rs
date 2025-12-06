use super::super::validation::{create_user_mask, is_banned};
use super::{ActorState, ChannelActor, ChannelMode, JoinSuccessData, MemberModes, Uid};
use crate::security::UserContext;
use slirc_proto::Message;
use std::collections::HashSet;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_join(
        &mut self,
        uid: Uid,
        nick: String,
        sender: mpsc::Sender<Message>,
        caps: HashSet<String>,
        user_context: UserContext,
        key_arg: Option<String>,
        initial_modes: Option<MemberModes>,
        join_msg_extended: Message,
        join_msg_standard: Message,
        session_id: Uuid,
        reply_tx: oneshot::Sender<Result<JoinSuccessData, String>>,
    ) {
        if self.state == ActorState::Draining {
            let _ = reply_tx.send(Err("ERR_CHANNEL_TOMBSTONE".to_string()));
            return;
        }

        // Validate that the user still exists and the session matches.
        let session_valid = if let Some(matrix) = self.matrix.upgrade() {
            if let Some(user_ref) = matrix.users.get(&uid) {
                let user = user_ref.read().await;
                user.session_id == session_id
            } else {
                false
            }
        } else {
            false
        };

        if !session_valid {
            let _ = reply_tx.send(Err("ERR_SESSION_INVALID".to_string()));
            return;
        }

        // Checks
        let user_mask = create_user_mask(&user_context);

        // 1. Bans (+b)
        if is_banned(&user_mask, &user_context, &self.bans, &self.excepts) {
            let _ = reply_tx.send(Err("ERR_BANNEDFROMCHAN".to_string()));
            return;
        }

        // 2. Invite Only (+i)
        if self.modes.contains(&ChannelMode::InviteOnly) {
            let is_invited = self.is_invited(&uid);
            let is_invex = self.invex.iter().any(|i| {
                crate::security::matches_ban_or_except(&i.mask, &user_mask, &user_context)
            });

            if !is_invited && !is_invex {
                let _ = reply_tx.send(Err("ERR_INVITEONLYCHAN".to_string()));
                return;
            }
        }

        // 3. Limit (+l)
        for mode in &self.modes {
            if let ChannelMode::Limit(limit) = mode
                && self.members.len() >= *limit
            {
                let _ = reply_tx.send(Err("ERR_CHANNELISFULL".to_string()));
                return;
            }
        }

        // 4. Key (+k)
        for mode in &self.modes {
            if let ChannelMode::Key(key) = mode
                && key_arg.as_deref() != Some(key)
            {
                let _ = reply_tx.send(Err("ERR_BADCHANNELKEY".to_string()));
                return;
            }
        }

        // Consume invite
        self.remove_invite(&uid);

        // Basic JOIN implementation
        // Fix #14: Preserve existing modes if user is already in channel (rejoin)
        let modes = if let Some(existing) = self.members.get(&uid) {
            existing.clone()
        } else {
            // Grant operator status to the first user (channel founder)
            let is_first_user = self.members.is_empty();
            if is_first_user {
                MemberModes {
                    op: true,
                    ..Default::default()
                }
            } else {
                initial_modes.unwrap_or_default()
            }
        };

        self.members.insert(uid.clone(), modes);
        self.user_nicks.insert(uid.clone(), nick.clone());
        self.senders.insert(uid.clone(), sender.clone());
        self.user_caps.insert(uid.clone(), caps.clone());

        self.handle_broadcast_with_cap(
            join_msg_extended,
            vec![uid.clone()],
            Some("extended-join".to_string()),
            Some(join_msg_standard),
        )
        .await;

        let is_secret = self.modes.contains(&ChannelMode::Secret);

        let data = JoinSuccessData {
            topic: self.topic.clone(),
            channel_name: self.name.clone(),
            is_secret,
        };

        let _ = reply_tx.send(Ok(data));
    }
}
