use super::{ChannelActor, ChannelMode, Uid};
use slirc_proto::mode::{ChannelMode as ProtoChannelMode, Mode};
use slirc_proto::{Command, Message, Prefix};
use std::collections::HashMap;
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
        reply_tx: oneshot::Sender<Result<Vec<Mode<ProtoChannelMode>>, String>>,
    ) {
        let mut applied_modes = Vec::new();

        // Basic permission check
        let sender_modes = self.members.get(&sender_uid).cloned().unwrap_or_default();
        let has_priv = sender_modes.has_op_or_higher() || force;

        for mode in modes {
            if !has_priv && !force {
                continue;
            }

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
                        Self::apply_list_mode(&mut self.bans, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Exception => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.excepts, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoChannelMode::InviteException => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.invex, mask, adding, &sender_uid)
                    } else {
                        false
                    }
                }
                ProtoChannelMode::Quiet => {
                    if let Some(mask) = arg {
                        Self::apply_list_mode(&mut self.quiets, mask, adding, &sender_uid)
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
                        if let Some(limit) = arg.and_then(|a| a.parse::<usize>().ok()) {
                            self.replace_param_mode(
                                |mode| matches!(mode, ChannelMode::Limit(_)),
                                Some(ChannelMode::Limit(limit)),
                            )
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
            }
        }

        if !applied_modes.is_empty() {
            let msg = Message {
                tags: None,
                prefix: Some(sender_prefix),
                command: Command::ChannelMODE(self.name.clone(), applied_modes.clone()),
            };
            for sender in self.senders.values() {
                let _ = sender.send(msg.clone()).await;
            }
        }

        let _ = reply_tx.send(Ok(applied_modes));
    }
}
