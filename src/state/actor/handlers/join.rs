//! JOIN event handling.
//!
//! Processes channel join requests with ban/invite/key validation.

use super::super::validation::{create_user_mask, is_banned};
use super::{
    ActorState, ChannelActor, ChannelError, ChannelMode, JoinParams, JoinSuccessData, MemberModes,
};
use slirc_proto::{Command, Message};
use tokio::sync::oneshot;
use tracing::debug;

impl ChannelActor {
    pub(crate) async fn handle_join(
        &mut self,
        params: JoinParams,
        reply_tx: oneshot::Sender<Result<JoinSuccessData, ChannelError>>,
    ) {
        let JoinParams {
            uid,
            nick,
            sender,
            caps,
            user_context,
            key: key_arg,
            initial_modes,
            join_msg_extended,
            join_msg_standard,
            session_id,
        } = params;

        if self.state == ActorState::Draining {
            let _ = reply_tx.send(Err(ChannelError::ChannelTombstone));
            return;
        }

        // Validate that the user still exists and the session matches.
        let session_valid = if let Some(matrix) = self.matrix.upgrade() {
            let user_arc = matrix
                .user_manager
                .users
                .get(&uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                user.session_id == session_id
            } else {
                false
            }
        } else {
            false
        };

        if !session_valid {
            let _ = reply_tx.send(Err(ChannelError::SessionInvalid));
            return;
        }

        // Checks
        let user_mask = create_user_mask(&user_context);

        // Check if user has an active invite (exempts from bans and invite-only)
        let is_invited = self.is_invited(&uid);

        // Check for invite exceptions (+I mode)
        let is_invex = self
            .invex
            .iter()
            .any(|i| crate::security::matches_ban_or_except(&i.mask, &user_mask, &user_context));

        // 1. Bans (+b) - invites and invex exempt from ban
        // CRITICAL FIX: is_invited MUST exempt from bans (RFC 2812, irctest testInviteExemptsFromBan)
        if !is_invex
            && !is_invited
            && is_banned(&user_mask, &user_context, &self.bans, &self.excepts)
        {
            let _ = reply_tx.send(Err(ChannelError::BannedFromChan));
            return;
        }

        // 2. Invite Only (+i)
        if self.modes.contains(&ChannelMode::InviteOnly) && !is_invited && !is_invex {
            let _ = reply_tx.send(Err(ChannelError::InviteOnlyChan));
            return;
        }

        // 3. Limit (+l) and Redirect (+L)
        for mode in &self.modes {
            if let ChannelMode::Limit(limit, _) = mode
                && self.members.len() >= *limit
            {
                // Check for redirect (+L)
                for l_mode in &self.modes {
                    if let ChannelMode::Redirect(target, _) = l_mode {
                        let _ = reply_tx.send(Err(ChannelError::Redirect(target.clone())));
                        return;
                    }
                }
                let _ = reply_tx.send(Err(ChannelError::ChannelIsFull));
                return;
            }
        }

        // 4. Key (+k)
        for mode in &self.modes {
            if let ChannelMode::Key(key, _) = mode
                && key_arg.as_deref() != Some(key)
            {
                let _ = reply_tx.send(Err(ChannelError::BadChannelKey));
                return;
            }
        }

        // 4b. Join Flood (+f)
        // 4b. Join Flood (+f)
        if let Some(limiter) = &self.flood_join_limiter {
            if limiter.check().is_err() {
                // Trigger protection: Set +i if not already set
                if !self.modes.contains(&ChannelMode::InviteOnly) {
                    self.set_flag_mode(ChannelMode::InviteOnly, true);
                    
                    // Broadcast mode change
                    let msg = Message {
                        tags: None,
                        prefix: Some(slirc_proto::Prefix::new(
                            self.server_id.to_string(),
                            "system".to_string(),
                            self.server_id.to_string(),
                        )),
                        command: Command::ChannelMODE(
                            self.name.clone(),
                            vec![slirc_proto::mode::Mode::plus(
                                slirc_proto::mode::ChannelMode::InviteOnly,
                                None
                            )]
                        ),
                    };
                    self.handle_broadcast(msg, None).await;
                    
                    // Also send a notice explaining why
                    let notice = Message {
                        tags: None,
                        prefix: Some(slirc_proto::Prefix::new(
                            self.server_id.to_string(),
                            "system".to_string(),
                            self.server_id.to_string(),
                        )),
                        command: Command::NOTICE(
                            self.name.clone(),
                            "Channel join flood detected. Invite-only mode enabled (+i).".to_string()
                        ),
                    };
                    self.handle_broadcast(notice, None).await;
                }
                
                let _ = reply_tx.send(Err(ChannelError::ChannelIsFull));
                return;
            }
        }

        // 5. RegisteredOnly (+R) - only users identified to NickServ can join
        if self.modes.contains(&ChannelMode::RegisteredOnly) && !user_context.is_registered {
            let _ = reply_tx.send(Err(ChannelError::NeedReggedNick));
            return;
        }

        // 6. TlsOnly (+z) - only TLS/SSL connections can join
        if self.modes.contains(&ChannelMode::TlsOnly) && !user_context.is_tls {
            let _ = reply_tx.send(Err(ChannelError::SecureOnlyChan));
            return;
        }

        // 7. OperOnly (+O) - only IRC operators can join
        if self.modes.contains(&ChannelMode::OperOnly) && !user_context.is_oper {
            let _ = reply_tx.send(Err(ChannelError::OperOnlyChan));
            return;
        }

        // 8. AdminOnly (+A) - only server admins can join
        // Uses oper_type to distinguish admin from regular oper.
        if self.modes.contains(&ChannelMode::AdminOnly)
            && user_context.oper_type.as_deref() != Some("admin")
        {
            let _ = reply_tx.send(Err(ChannelError::AdminOnlyChan));
            return;
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

        // Update channel member count metric (Innovation 3)
        crate::metrics::set_channel_members(&self.name, self.members.len() as i64);

        // Determine visibility for Auditorium mode (+u)
        let mut exclude = vec![uid.clone()];
        let is_auditorium = self.modes.contains(&ChannelMode::Auditorium);

        // Check if joiner is privileged
        let joiner_privileged = self
            .members
            .get(&uid)
            .map(|m| m.voice || m.halfop || m.op || m.admin || m.owner)
            .unwrap_or(false);

        if is_auditorium && !joiner_privileged {
            debug!(
                "Auditorium mode active. Joiner {} is not privileged. Calculating exclusions.",
                uid
            );
            // If +u and joiner is not privileged, only privileged members see the JOIN.
            // So we exclude all non-privileged members.
            for (member_uid, modes) in &self.members {
                if !modes.voice && !modes.halfop && !modes.op && !modes.admin && !modes.owner {
                    // Don't exclude the joiner again (already in exclude)
                    if member_uid != &uid {
                        debug!(
                            "Excluding non-privileged member {} from seeing join of {}",
                            member_uid, uid
                        );
                        exclude.push(member_uid.clone());
                    }
                } else {
                    debug!(
                        "Member {} is privileged (modes={:?}), allowing join visibility",
                        member_uid, modes
                    );
                }
            }
        } else {
            debug!(
                "Auditorium check skipped. is_auditorium={}, joiner_privileged={}",
                is_auditorium, joiner_privileged
            );
        }

        // 9. Delayed Join (+D) - don't broadcast join message
        let is_delayed = self.modes.contains(&ChannelMode::DelayedJoin);

        if !is_delayed {
            self.handle_broadcast_with_cap(
                join_msg_extended.clone(),
                exclude,
                Some("extended-join".to_string()),
                Some(join_msg_standard.clone()),
            )
            .await;

            // Store JOIN event in history (EventPlayback)
            if let Some(matrix) = self.matrix.upgrade() {
                let event_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
                let source_prefix = format!("{}!{}@{}", nick, user_context.username, user_context.hostname);

                let event = crate::history::types::HistoryItem::Event(crate::history::types::StoredEvent {
                    id: event_id,
                    nanotime: now,
                    source: source_prefix,
                    kind: crate::history::types::EventKind::Join,
                });

                let history = matrix.service_manager.history.clone();
                let target = self.name.clone();
                tokio::spawn(async move {
                    let _ = history.store_item(&target, event).await;
                });
            }
        } else {
            // Joiner is silent until they speak
            self.silent_members.insert(uid.clone());
        }

        let is_secret = self.modes.contains(&ChannelMode::Secret);

        let data = JoinSuccessData {
            topic: self.topic.clone(),
            channel_name: self.name.clone(),
            is_secret,
        };

        self.notify_observer(None);
        let _ = reply_tx.send(Ok(data));
    }
}
