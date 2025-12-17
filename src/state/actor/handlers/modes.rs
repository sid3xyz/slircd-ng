//! MODE event handling for channels.
//!
//! Applies mode changes with privilege validation and broadcasts results.

use super::{ChannelActor, ChannelError, ChannelMode, Uid, ClearTarget};
use slirc_proto::mode::{ChannelMode as ProtoChannelMode, Mode};
use slirc_proto::{Command, Message, Prefix};
use std::collections::HashMap;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_apply_modes(
        &mut self,
        sender_uid: Uid,
        sender_prefix: Prefix,
        modes: Vec<Mode<ProtoChannelMode>>,
        target_uids: HashMap<String, Uid>,
        force: bool,
        reply_tx: oneshot::Sender<Result<Vec<Mode<ProtoChannelMode>>, ChannelError>>,
    ) {
        let mut applied_modes = Vec::new();

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
                ProtoChannelMode::Ban => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.bans, mask, adding, &sender_prefix.to_string())
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Exception => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.excepts, mask, adding, &sender_prefix.to_string())
                    } else {
                        false
                    }
                }
                ProtoChannelMode::InviteException => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.invex, mask, adding, &sender_prefix.to_string())
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Quiet => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.quiets, mask, adding, &sender_prefix.to_string())
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Key => {
                    if adding {
                        if let Some(key) = arg {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Key(_)),
                                Some(ChannelMode::Key(key.to_string())),
                            )
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Key(_)), None)
                    }
                }

                ProtoChannelMode::Limit => {
                    if adding {
                        // Parse and validate limit: must be positive and reasonable
                        if let Some(limit) = arg.and_then(|a| a.parse::<usize>().ok()) {
                            // Reject unreasonable limits (0 or absurdly high values)
                            // Maximum of 10000 members per channel is generous
                            if limit == 0 || limit > 10000 {
                                false
                            } else {
                                self.replace_param_mode(
                                    |mode| matches!(mode, ChannelMode::Limit(_)),
                                    Some(ChannelMode::Limit(limit)),
                                )
                            }
                        } else {
                            false
                        }
                    } else {
                        self.replace_param_mode(|mode| matches!(mode, ChannelMode::Limit(_)), None)
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
            let msg = Message {
                tags: None,
                prefix: Some(sender_prefix),
                command: Command::ChannelMODE(self.name.clone(), applied_modes.clone()),
            };
            for (uid, sender) in &self.senders {
                if let Err(err) = sender.try_send(msg.clone()) {
                    match err {
                        TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                        TrySendError::Closed(_) => {}
                    }
                }
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
        let mut changes = Vec::new();

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
                    command: Command::NOTICE(self.name.clone(), "Channel modes cleared".to_string()),
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
             }
        }

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
