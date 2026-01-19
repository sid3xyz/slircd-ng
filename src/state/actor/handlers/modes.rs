//! MODE event handling for channels.
//!
//! Applies mode changes with privilege validation and broadcasts results.

use super::{ChannelActor, ChannelError, ChannelMode, ClearTarget, ModeParams, Uid};
use slirc_proto::mode::{ChannelMode as ProtoChannelMode, Mode};
use slirc_proto::sync::clock::HybridTimestamp;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

/// Parse and validate a channel limit argument.
/// Returns `Some(limit)` if valid (1..=10000), `None` otherwise.
fn parse_channel_limit(arg: Option<&str>) -> Option<usize> {
    let limit = arg?.parse::<usize>().ok()?;
    // Reject zero or absurdly high values (max 10000 members per channel)
    if limit == 0 || limit > 10000 {
        return None;
    }
    Some(limit)
}

impl ChannelActor {
    pub(crate) async fn handle_apply_modes(
        &mut self,
        params: ModeParams,
        reply_tx: oneshot::Sender<Result<Vec<Mode<ProtoChannelMode>>, ChannelError>>,
    ) {
        let ModeParams {
            sender_uid,
            sender_prefix,
            modes,
            target_uids,
            force,
        } = params;

        let mut applied_modes = Vec::with_capacity(modes.len());

        // Basic permission check
        let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
        let has_priv = sender_modes.has_op_or_higher() || force;

        if !has_priv {
            let _ = reply_tx.send(Err(ChannelError::ChanOpPrivsNeeded));
            return;
        }

        for mode in modes {
            let adding = mode.is_plus();
            let mode_type = mode.mode();
            let arg = mode.arg();

            let changed = match mode_type {
                ProtoChannelMode::NoExternalMessages => {
                    self.set_flag_mode(ChannelMode::NoExternal, adding)
                }
                ProtoChannelMode::ProtectedTopic => {
                    self.set_flag_mode(ChannelMode::TopicLock, adding)
                }
                ProtoChannelMode::InviteOnly => self.set_flag_mode(ChannelMode::InviteOnly, adding),
                ProtoChannelMode::Moderated => self.set_flag_mode(ChannelMode::Moderated, adding),
                ProtoChannelMode::ModeratedUnreg => {
                    self.set_flag_mode(ChannelMode::ModeratedUnreg, adding)
                }
                ProtoChannelMode::OpModerated => {
                    self.set_flag_mode(ChannelMode::OpModerated, adding)
                }
                ProtoChannelMode::Auditorium => self.set_flag_mode(ChannelMode::Auditorium, adding),
                ProtoChannelMode::Secret => self.set_flag_mode(ChannelMode::Secret, adding),
                ProtoChannelMode::RegisteredOnly => {
                    self.set_flag_mode(ChannelMode::RegisteredOnly, adding)
                }
                ProtoChannelMode::NoColors => self.set_flag_mode(ChannelMode::NoColors, adding),
                ProtoChannelMode::NoCTCP => self.set_flag_mode(ChannelMode::NoCtcp, adding),
                ProtoChannelMode::NoNickChange => {
                    self.set_flag_mode(ChannelMode::NoNickChange, adding)
                }
                ProtoChannelMode::NoKnock => self.set_flag_mode(ChannelMode::NoKnock, adding),
                ProtoChannelMode::NoInvite => self.set_flag_mode(ChannelMode::NoInvite, adding),
                ProtoChannelMode::NoChannelNotice => {
                    self.set_flag_mode(ChannelMode::NoNotice, adding)
                }
                ProtoChannelMode::NoKick => self.set_flag_mode(ChannelMode::NoKicks, adding),
                ProtoChannelMode::Permanent => self.set_flag_mode(ChannelMode::Permanent, adding),
                ProtoChannelMode::OperOnly => self.set_flag_mode(ChannelMode::OperOnly, adding),
                ProtoChannelMode::FreeInvite => self.set_flag_mode(ChannelMode::FreeInvite, adding),
                ProtoChannelMode::TlsOnly => self.set_flag_mode(ChannelMode::TlsOnly, adding),
                ProtoChannelMode::Roleplay => self.set_flag_mode(ChannelMode::Roleplay, adding),
                ProtoChannelMode::DelayedJoin => self.set_flag_mode(ChannelMode::DelayedJoin, adding),
                ProtoChannelMode::StripColors => self.set_flag_mode(ChannelMode::StripColors, adding),
                ProtoChannelMode::AntiCaps => self.set_flag_mode(ChannelMode::AntiCaps, adding),
                ProtoChannelMode::Censor => self.set_flag_mode(ChannelMode::Censor, adding),
                ProtoChannelMode::Ban => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(
                            &mut self.bans,
                            mask,
                            adding,
                            &sender_prefix.to_string(),
                        )
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Exception => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(
                            &mut self.excepts,
                            mask,
                            adding,
                            &sender_prefix.to_string(),
                        )
                    } else {
                        false
                    }
                }
                ProtoChannelMode::InviteException => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(
                            &mut self.invex,
                            mask,
                            adding,
                            &sender_prefix.to_string(),
                        )
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Quiet => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(
                            &mut self.quiets,
                            mask,
                            adding,
                            &sender_prefix.to_string(),
                        )
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Key => {
                    if adding {
                        if let Some(key) = arg {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Key(_, _)),
                                Some(ChannelMode::Key(
                                    key.to_string(),
                                    HybridTimestamp::now(&self.server_id),
                                )),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Key(_, _)), None)
                    }
                }

                ProtoChannelMode::Limit => {
                    if adding {
                        parse_channel_limit(arg).is_some_and(|limit| {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Limit(_, _)),
                                Some(ChannelMode::Limit(
                                    limit,
                                    HybridTimestamp::now(&self.server_id),
                                )),
                            )
                        })
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Limit(_, _)), None)
                    }
                }
                ProtoChannelMode::JoinForward => {
                    if adding {
                        if let Some(target) = arg {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::JoinForward(_, _)),
                                Some(ChannelMode::JoinForward(
                                    target.to_string(),
                                    HybridTimestamp::now(&self.server_id),
                                )),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(
                            |mode| matches!(mode, ChannelMode::JoinForward(_, _)),
                            None,
                        )
                    }
                }
                ProtoChannelMode::Flood => {
                    use std::str::FromStr;
                    use governor::{Quota, RateLimiter};
                    use std::num::NonZeroU32;

                    if adding {
                        if let Some(param_str) = arg {
                            let mut valid_params = Vec::new();

                            // Support comma-separated list: "5j:10,3m:5"
                            for part in param_str.split(',') {
                                if let Ok(param) = super::FloodParam::from_str(part) {
                                    valid_params.push(param);
                                }
                            }

                            if !valid_params.is_empty() {
                                // If replacing, we should probably merge or overwrite?
                                // Standard behavior: +f overwrites all flood settings with the new string.
                                self.flood_config.clear();
                                self.flood_message_limiters.clear();
                                self.flood_join_limiter = None;

                                for param in &valid_params {
                                    self.flood_config.insert(param.type_, *param);

                                    match param.type_ {
                                        super::FloodType::Message => {
                                             // Message limiters are created per-user on demand in message handler
                                             // We just cleared the map, so they will be recreated with new policy.
                                        }
                                        super::FloodType::Join => {
                                             // Calculate period per join allowed
                                             // period (secs) / count (joins)
                                             let period_per_action = std::time::Duration::from_secs_f64(
                                                 param.period as f64 / param.count as f64
                                             );
                                             
                                             if let Some(quota) = Quota::with_period(period_per_action) {
                                                 let quota = quota.allow_burst(NonZeroU32::new(param.count).unwrap());
                                                 self.flood_join_limiter = Some(RateLimiter::direct(quota));
                                             }
                                        }
                                    }
                                }

                                // Reconstruct canonical string
                                let mut parts: Vec<String> = self.flood_config.values().map(|p| p.to_string()).collect();
                                parts.sort(); // Deterministic order
                                let canonical = parts.join(",");

                                self.replace_param_mode(
                                    |mode| matches!(mode, ChannelMode::Flood(_, _)),
                                    Some(ChannelMode::Flood(
                                        canonical,
                                        HybridTimestamp::now(&self.server_id),
                                    )),
                                )
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        // Remove -f
                        self.flood_config.clear();
                        self.flood_message_limiters.clear();
                        self.flood_join_limiter = None;
                        self.replace_param_mode(
                            |mode| matches!(mode, ChannelMode::Flood(_, _)),
                            None,
                        )
                    }
                }
                ProtoChannelMode::Redirect => {
                    if adding {
                        if let Some(target) = arg {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Redirect(_, _)),
                                Some(ChannelMode::Redirect(
                                    target.to_string(),
                                    HybridTimestamp::now(&self.server_id),
                                )),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(
                            |mode| matches!(mode, ChannelMode::Redirect(_, _)),
                            None,
                        )
                    }
                }
                ProtoChannelMode::Founder => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.owner = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Admin => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.admin = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Oper => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.op = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Halfop => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.halfop = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Voice => {
                    if let Some(nick) = arg {
                        if let Some(target_uid) = target_uids.get(nick) {
                            self.update_member_mode(target_uid, |m| m.voice = adding)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if changed {
                applied_modes.push(mode.clone());

                // Record mode change metric (Innovation 3)
                let mode_char = proto_mode_to_char(mode_type);
                crate::metrics::record_mode_change(mode_char);
            }
        }

        if !applied_modes.is_empty() {
            let msg = Arc::new(Message {
                tags: None,
                prefix: Some(sender_prefix.clone()),
                command: Command::ChannelMODE(self.name.clone(), applied_modes.clone()),
            });
            for (uid, sender) in &self.senders {
                if let Err(err) = sender.try_send(msg.clone()) {
                    match err {
                        TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                        TrySendError::Closed(_) => {}
                    }
                }
            }
            self.notify_observer(None);

            // Store MODE event in history (EventPlayback)
            if let Some(matrix) = self.matrix.upgrade() {
                // Serialize modes to string
                let mut mode_str = String::new();
                let mut args = Vec::new();
                let mut current_group_sign = None;
                 
                for mode in &applied_modes {
                     let is_plus = mode.is_plus();
                     if current_group_sign != Some(is_plus) {
                         mode_str.push(if is_plus { '+' } else { '-' });
                         current_group_sign = Some(is_plus);
                     }
                     mode_str.push(proto_mode_to_char(&mode.mode()));
                     if let Some(arg) = mode.arg() {
                         args.push(arg.to_string());
                     }
                }
                 
                if !args.is_empty() {
                     mode_str.push(' ');
                     mode_str.push_str(&args.join(" "));
                }

                let event_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
                let source = sender_prefix.to_string();

                let event = crate::history::types::HistoryItem::Event(crate::history::types::StoredEvent {
                    id: event_id,
                    nanotime: now,
                    source,
                    kind: crate::history::types::EventKind::Mode { diff: mode_str },
                });

                let history = matrix.service_manager.history.clone();
                let target = self.name.clone();
                tokio::spawn(async move {
                    let _ = history.store_item(&target, event).await;
                });
            }
        }

        let _ = reply_tx.send(Ok(applied_modes));
    }

    pub(crate) async fn handle_clear(
        &mut self,
        _sender_uid: Uid,
        sender_prefix: Prefix,
        target: ClearTarget,
        reply_tx: oneshot::Sender<Result<(), ChannelError>>,
    ) {
        let mut changes = Vec::with_capacity(8);

        match target {
            ClearTarget::Modes => {
                // Reset all modes to default
                self.modes.clear();
                self.modes.insert(ChannelMode::NoExternal);
                self.modes.insert(ChannelMode::TopicLock);

                // Notify about mode clear
                let msg = Message {
                    tags: None,
                    prefix: Some(sender_prefix.clone()),
                    command: Command::NOTICE(
                        self.name.clone(),
                        "Channel modes cleared".to_string(),
                    ),
                };
                self.handle_broadcast(msg, None).await;
            }
            ClearTarget::Bans => {
                self.bans.clear();
                self.excepts.clear();
                self.invex.clear();
                self.quiets.clear();

                let msg = Message {
                    tags: None,
                    prefix: Some(sender_prefix.clone()),
                    command: Command::NOTICE(self.name.clone(), "Channel bans cleared".to_string()),
                };
                self.handle_broadcast(msg, None).await;
            }
            ClearTarget::Ops => {
                for (uid, modes) in self.members.iter_mut() {
                    if modes.op {
                        modes.op = false;
                        if let Some(nick) = self.user_nicks.get(uid) {
                            changes.push(Mode::minus(ProtoChannelMode::Oper, Some(nick.as_str())));
                        }
                    }
                }
            }
            ClearTarget::Voices => {
                for (uid, modes) in self.members.iter_mut() {
                    if modes.voice {
                        modes.voice = false;
                        if let Some(nick) = self.user_nicks.get(uid) {
                            changes.push(Mode::minus(ProtoChannelMode::Voice, Some(nick.as_str())));
                        }
                    }
                }
            }
        }

        // Broadcast changes if any
        if !changes.is_empty() {
            // Batch into messages of max 12 modes (standard limit)
            for chunk in changes.chunks(12) {
                let msg = Message {
                    tags: None,
                    prefix: Some(sender_prefix.clone()),
                    command: Command::ChannelMODE(self.name.clone(), chunk.to_vec()),
                };
                self.handle_broadcast(msg, None).await;

                // Store MODE event in history (EventPlayback)
                if let Some(matrix) = self.matrix.upgrade() {
                    // Serialize modes to string
                    let mut mode_str = String::new();
                    let mut args = Vec::new();
                    let mut current_group_sign = None;
                    
                    for mode in chunk {
                        let is_plus = mode.is_plus();
                        if current_group_sign != Some(is_plus) {
                            mode_str.push(if is_plus { '+' } else { '-' });
                            current_group_sign = Some(is_plus);
                        }
                        mode_str.push(proto_mode_to_char(&mode.mode()));
                        if let Some(arg) = mode.arg() {
                            args.push(arg.to_string());
                        }
                    }
                    
                    if !args.is_empty() {
                        mode_str.push(' ');
                        mode_str.push_str(&args.join(" "));
                    }

                    let event_id = uuid::Uuid::new_v4().to_string();
                    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
                    let source = sender_prefix.to_string();

                    let event = crate::history::types::HistoryItem::Event(crate::history::types::StoredEvent {
                        id: event_id,
                        nanotime: now,
                        source,
                        kind: crate::history::types::EventKind::Mode { diff: mode_str },
                    });

                    let history = matrix.service_manager.history.clone();
                    let target = self.name.clone();
                    tokio::spawn(async move {
                        let _ = history.store_item(&target, event).await;
                    });
                }
            }
        }

        self.notify_observer(None);
        let _ = reply_tx.send(Ok(()));
    }
}

/// Convert a protocol channel mode to its character representation.
fn proto_mode_to_char(mode: &ProtoChannelMode) -> char {
    match mode {
        ProtoChannelMode::NoExternalMessages => 'n',
        ProtoChannelMode::ProtectedTopic => 't',
        ProtoChannelMode::InviteOnly => 'i',
        ProtoChannelMode::Moderated => 'm',
        ProtoChannelMode::Secret => 's',
        ProtoChannelMode::RegisteredOnly => 'r',
        ProtoChannelMode::NoColors => 'c',
        ProtoChannelMode::NoCTCP => 'C',
        ProtoChannelMode::NoNickChange => 'N',
        ProtoChannelMode::NoKnock => 'K',
        ProtoChannelMode::NoInvite => 'V',
        ProtoChannelMode::NoChannelNotice => 'T',
        ProtoChannelMode::NoKick => 'Q',
        ProtoChannelMode::Permanent => 'P',
        ProtoChannelMode::OperOnly => 'O',
        ProtoChannelMode::FreeInvite => 'g',
        ProtoChannelMode::TlsOnly => 'z',
        ProtoChannelMode::Roleplay => 'E',
        ProtoChannelMode::DelayedJoin => 'D',
        ProtoChannelMode::StripColors => 'S',
        ProtoChannelMode::AntiCaps => 'B',
        ProtoChannelMode::Censor => 'G',
        ProtoChannelMode::Redirect => 'L',
        ProtoChannelMode::Flood => 'f',
        ProtoChannelMode::JoinForward => 'F',
        ProtoChannelMode::Ban => 'b',
        ProtoChannelMode::Exception => 'e',
        ProtoChannelMode::InviteException => 'I',
        ProtoChannelMode::Quiet => 'q',
        ProtoChannelMode::Key => 'k',
        ProtoChannelMode::Limit => 'l',
        ProtoChannelMode::Founder => 'q',
        ProtoChannelMode::Admin => 'a',
        ProtoChannelMode::Oper => 'o',
        ProtoChannelMode::Halfop => 'h',
        ProtoChannelMode::Voice => 'v',
        _ => '?',
    }
}
