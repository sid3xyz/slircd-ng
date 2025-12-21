#![allow(clippy::collapsible_if)]
use super::{ChannelActor, ChannelMode, Uid};
use crate::state::{ListEntry, Topic};
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use slirc_proto::mode::ModeType;
use slirc_proto::{ChannelMode as ProtoChannelMode, Mode};

impl ChannelActor {
    pub(crate) async fn handle_remote_mode(
        &mut self,
        ts: u64,
        setter: String,
        modes_str: String,
        params: Vec<String>,
    ) {
        // TS check
        if self.created > 0 {
            if (ts as i64) < self.created {
                self.created = ts as i64;
            }
        } else {
            self.created = ts as i64;
        }

        // Parse modes
        let modes = parse_modes(modes_str, &params);

        for mode in modes {
            match mode {
                Mode::Plus(ref m, _) => {
                    let arg = mode.arg().map(|s| s.to_string());
                    self.apply_remote_mode_add(m.clone(), arg, &setter, ts);
                }
                Mode::Minus(ref m, _) => {
                    let arg = mode.arg().map(|s| s.to_string());
                    self.apply_remote_mode_remove(m.clone(), arg, &setter);
                }
                _ => {}
            }
        }
    }

    pub(crate) async fn handle_remote_topic(&mut self, ts: u64, setter: String, topic: String) {
        let should_update = if let Some(current_ts) = &self.topic_timestamp {
            ts > current_ts.millis as u64
        } else {
            true
        };

        if should_update {
            self.topic = Some(Topic {
                text: topic,
                set_by: setter,
                set_at: ts as i64,
            });
            self.topic_timestamp = Some(HybridTimestamp::new(
                ts as i64 * 1000,
                0,
                &ServerId::new("000"),
            ));
        }
    }

    pub(crate) async fn handle_remote_kick(
        &mut self,
        _sender: String,
        target: Uid,
        _reason: Option<String>,
    ) {
        if self.members.remove(&target).is_some() {
            if self.members.is_empty() && !self.modes.contains(&ChannelMode::Permanent) {
                // Channel empty logic if needed
            }
        }
    }

    fn apply_remote_mode_add(
        &mut self,
        mode: ProtoChannelMode,
        arg: Option<String>,
        setter: &str,
        ts: u64,
    ) {
        let ts_obj = HybridTimestamp::new(ts as i64 * 1000, 0, &ServerId::new("000"));

        match mode {
            ProtoChannelMode::Ban => {
                if let Some(mask) = arg {
                    self.bans.push(ListEntry {
                        mask,
                        set_by: setter.to_string(),
                        set_at: ts as i64,
                    });
                }
            }
            ProtoChannelMode::Exception => {
                if let Some(mask) = arg {
                    self.excepts.push(ListEntry {
                        mask,
                        set_by: setter.to_string(),
                        set_at: ts as i64,
                    });
                }
            }
            ProtoChannelMode::InviteException => {
                if let Some(mask) = arg {
                    self.invex.push(ListEntry {
                        mask,
                        set_by: setter.to_string(),
                        set_at: ts as i64,
                    });
                }
            }
            ProtoChannelMode::Limit => {
                if let Some(l) = arg.and_then(|s| s.parse().ok()) {
                    self.modes
                        .retain(|m| !matches!(m, ChannelMode::Limit(_, _)));
                    self.modes.insert(ChannelMode::Limit(l, ts_obj));
                }
            }
            ProtoChannelMode::Key => {
                if let Some(k) = arg {
                    self.modes.retain(|m| !matches!(m, ChannelMode::Key(_, _)));
                    self.modes.insert(ChannelMode::Key(k, ts_obj));
                }
            }
            // Simple modes
            ProtoChannelMode::InviteOnly => {
                self.modes.insert(ChannelMode::InviteOnly);
            }
            ProtoChannelMode::Moderated => {
                self.modes.insert(ChannelMode::Moderated);
            }
            ProtoChannelMode::NoExternalMessages => {
                self.modes.insert(ChannelMode::NoExternal);
            }
            ProtoChannelMode::Secret => {
                self.modes.insert(ChannelMode::Secret);
            }
            ProtoChannelMode::ProtectedTopic => {
                self.modes.insert(ChannelMode::TopicLock);
            }
            ProtoChannelMode::NoColors => {
                self.modes.insert(ChannelMode::NoColors);
            }
            // Prefix modes
            ProtoChannelMode::Oper | ProtoChannelMode::Voice => {
                if let Some(uid) = arg {
                    if let Some(member) = self.members.get_mut(&uid) {
                        match mode {
                            ProtoChannelMode::Oper => member.op = true,
                            ProtoChannelMode::Voice => member.voice = true,
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_remote_mode_remove(
        &mut self,
        mode: ProtoChannelMode,
        arg: Option<String>,
        _setter: &str,
    ) {
        match mode {
            ProtoChannelMode::Ban => {
                if let Some(mask) = arg {
                    self.bans.retain(|e| e.mask != mask);
                }
            }
            ProtoChannelMode::Exception => {
                if let Some(mask) = arg {
                    self.excepts.retain(|e| e.mask != mask);
                }
            }
            ProtoChannelMode::InviteException => {
                if let Some(mask) = arg {
                    self.invex.retain(|e| e.mask != mask);
                }
            }
            ProtoChannelMode::Limit => {
                self.modes
                    .retain(|m| !matches!(m, ChannelMode::Limit(_, _)));
            }
            ProtoChannelMode::Key => {
                self.modes.retain(|m| !matches!(m, ChannelMode::Key(_, _)));
            }
            // Simple modes
            ProtoChannelMode::InviteOnly => {
                self.modes.remove(&ChannelMode::InviteOnly);
            }
            ProtoChannelMode::Moderated => {
                self.modes.remove(&ChannelMode::Moderated);
            }
            ProtoChannelMode::NoExternalMessages => {
                self.modes.remove(&ChannelMode::NoExternal);
            }
            ProtoChannelMode::Secret => {
                self.modes.remove(&ChannelMode::Secret);
            }
            ProtoChannelMode::ProtectedTopic => {
                self.modes.remove(&ChannelMode::TopicLock);
            }
            ProtoChannelMode::NoColors => {
                self.modes.remove(&ChannelMode::NoColors);
            }
            // Prefix modes
            ProtoChannelMode::Oper | ProtoChannelMode::Voice => {
                if let Some(uid) = arg {
                    if let Some(member) = self.members.get_mut(&uid) {
                        match mode {
                            ProtoChannelMode::Oper => member.op = false,
                            ProtoChannelMode::Voice => member.voice = false,
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// Helper to parse modes locally since slirc-proto helper is private
fn parse_modes(modes_str: String, params: &[String]) -> Vec<Mode<ProtoChannelMode>> {
    let mut result = Vec::new();
    let mut adding = true;
    let mut param_idx = 0;

    for c in modes_str.chars() {
        match c {
            '+' => adding = true,
            '-' => adding = false,
            _ => {
                let mode_type = ProtoChannelMode::from_char(c);
                let takes_arg = matches!(
                    mode_type,
                    ProtoChannelMode::Ban
                        | ProtoChannelMode::Exception
                        | ProtoChannelMode::InviteException
                        | ProtoChannelMode::Limit
                        | ProtoChannelMode::Key
                        | ProtoChannelMode::Oper
                        | ProtoChannelMode::Voice
                        | ProtoChannelMode::Founder
                        | ProtoChannelMode::Admin
                        | ProtoChannelMode::Halfop
                );

                // Special case: Limit only takes arg when adding
                let actual_takes_arg = if mode_type == ProtoChannelMode::Limit && !adding {
                    false
                } else {
                    takes_arg
                };

                let arg = if actual_takes_arg {
                    if param_idx < params.len() {
                        let a = params[param_idx].clone();
                        param_idx += 1;
                        Some(a)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Filter out unknown modes or unhandled ones
                if let ProtoChannelMode::Unknown(_) = mode_type {
                    continue;
                }

                if adding {
                    result.push(Mode::Plus(mode_type, arg));
                } else {
                    result.push(Mode::Minus(mode_type, arg));
                }
            }
        }
    }
    result
}
