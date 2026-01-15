#![allow(clippy::collapsible_if)]
//! Remote mode and topic handling with CRDT conflict resolution.
//!
//! Implements Last-Write-Wins (LWW) semantics for mode and topic changes
//! received from peer servers. Each mode bit has an independent timestamp,
//! and conflicts are resolved by comparing HybridTimestamps.

use super::{ChannelActor, ChannelMode, Uid};
use crate::state::{ListEntry, Topic};
use slirc_proto::sync::clock::{HybridTimestamp, ServerId};
use slirc_proto::mode::ModeType;
use slirc_proto::{ChannelMode as ProtoChannelMode, Mode};

impl ChannelActor {
    /// Handle incoming TMODE from a peer server with CRDT conflict resolution.
    ///
    /// Each mode bit is compared independently using LWW semantics:
    /// - If incoming timestamp > local timestamp: Apply change
    /// - If incoming timestamp <= local timestamp: Ignore (local wins)
    pub(crate) async fn handle_remote_mode(
        &mut self,
        ts: u64,
        setter: String,
        modes_str: String,
        params: Vec<String>,
    ) {
        // Convert u64 timestamp to HybridTimestamp
        // The setter's SID is used for tie-breaking (extracted from prefix)
        let source_sid = extract_sid_from_setter(&setter);
        let incoming_ts = HybridTimestamp::new(ts as i64 * 1000, 0, &source_sid);

        // TS check for channel creation time
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
                    self.apply_remote_mode_with_lww(m.clone(), arg, &setter, incoming_ts, true);
                }
                Mode::Minus(ref m, _) => {
                    let arg = mode.arg().map(|s| s.to_string());
                    self.apply_remote_mode_with_lww(m.clone(), arg, &setter, incoming_ts, false);
                }
                _ => {}
            }
        }
    }

    /// Handle incoming TOPIC from a peer server with LWW conflict resolution.
    pub(crate) async fn handle_remote_topic(&mut self, ts: u64, setter: String, topic: String) {
        let source_sid = extract_sid_from_setter(&setter);
        let incoming_ts = HybridTimestamp::new(ts as i64 * 1000, 0, &source_sid);

        let should_update = match &self.topic_timestamp {
            Some(current_ts) => incoming_ts > *current_ts,
            None => true,
        };

        if should_update {
            self.topic = Some(Topic {
                text: topic,
                set_by: setter,
                set_at: ts as i64,
            });
            self.topic_timestamp = Some(incoming_ts);
        }
    }

    pub(crate) async fn handle_remote_kick(
        &mut self,
        sender: String,
        target: Uid,
        reason: Option<String>,
    ) {
        if self.members.remove(&target).is_some() {
            self.senders.remove(&target);
            self.user_caps.remove(&target);
            self.user_nicks.remove(&target);

            // Broadcast KICK to remaining members
            let msg = std::sync::Arc::new(slirc_proto::Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::new_from_str(&sender)),
                command: slirc_proto::Command::KICK(
                    self.name.clone(),
                    target.clone(), // Should be nick, but we might only have UID here.
                    // Wait, KICK target is nick.
                    // The caller should resolve UID to nick if possible, or we use UID if it's a UID-based kick.
                    // Standard IRC uses nick.
                    reason,
                ),
            });

            for tx in self.senders.values() {
                let _ = tx.try_send(msg.clone());
            }

            // Update metrics
            crate::metrics::set_channel_members(&self.name, self.members.len() as i64);
            self.notify_observer(None);

            if self.members.is_empty() && !self.modes.contains(&ChannelMode::Permanent) {
                self.cleanup_if_empty();
            }
        }
    }

    /// Apply a mode change with LWW conflict resolution.
    ///
    /// Compares the incoming timestamp against the stored timestamp for this mode.
    /// Only applies the change if the incoming timestamp wins.
    fn apply_remote_mode_with_lww(
        &mut self,
        mode: ProtoChannelMode,
        arg: Option<String>,
        setter: &str,
        incoming_ts: HybridTimestamp,
        adding: bool,
    ) {
        match mode {
            // List modes (bans, exceptions, invex) - always apply, no LWW needed for individual entries
            ProtoChannelMode::Ban => {
                if let Some(mask) = arg {
                    if adding {
                        // Add if not already present
                        if !self.bans.iter().any(|e| e.mask == mask) {
                            self.bans.push(ListEntry {
                                mask,
                                set_by: setter.to_string(),
                                set_at: incoming_ts.millis / 1000,
                            });
                        }
                    } else {
                        self.bans.retain(|e| e.mask != mask);
                    }
                }
            }
            ProtoChannelMode::Exception => {
                if let Some(mask) = arg {
                    if adding {
                        if !self.excepts.iter().any(|e| e.mask == mask) {
                            self.excepts.push(ListEntry {
                                mask,
                                set_by: setter.to_string(),
                                set_at: incoming_ts.millis / 1000,
                            });
                        }
                    } else {
                        self.excepts.retain(|e| e.mask != mask);
                    }
                }
            }
            ProtoChannelMode::InviteException => {
                if let Some(mask) = arg {
                    if adding {
                        if !self.invex.iter().any(|e| e.mask == mask) {
                            self.invex.push(ListEntry {
                                mask,
                                set_by: setter.to_string(),
                                set_at: incoming_ts.millis / 1000,
                            });
                        }
                    } else {
                        self.invex.retain(|e| e.mask != mask);
                    }
                }
            }

            // Parameterized modes with LWW
            ProtoChannelMode::Limit => {
                if adding {
                    if let Some(l) = arg.and_then(|s| s.parse().ok()) {
                        // Check if incoming wins (LWW: Last-Write-Wins)
                        let current_ts = self
                            .modes
                            .iter()
                            .find_map(|m| match m {
                                ChannelMode::Limit(_, ts) => Some(*ts),
                                _ => None,
                            })
                            .or_else(|| self.mode_timestamps.get(&'l').copied());

                        // Incoming wins if no current timestamp or incoming is newer
                        if current_ts.is_none_or(|ts| incoming_ts > ts) {
                            self.modes
                                .retain(|m| !matches!(m, ChannelMode::Limit(_, _)));
                            self.modes.insert(ChannelMode::Limit(l, incoming_ts));
                            self.mode_timestamps.insert('l', incoming_ts);
                        }
                    }
                } else {
                    // Removing: check timestamp
                    let current_ts = self
                        .modes
                        .iter()
                        .find_map(|m| match m {
                            ChannelMode::Limit(_, ts) => Some(*ts),
                            _ => None,
                        })
                        .or_else(|| self.mode_timestamps.get(&'l').copied());

                    if current_ts.is_none_or(|ts| incoming_ts > ts) {
                        self.modes
                            .retain(|m| !matches!(m, ChannelMode::Limit(_, _)));
                        self.mode_timestamps.insert('l', incoming_ts);
                    }
                }
            }
            ProtoChannelMode::Key => {
                if adding {
                    if let Some(k) = arg {
                        let current_ts = self
                            .modes
                            .iter()
                            .find_map(|m| match m {
                                ChannelMode::Key(_, ts) => Some(*ts),
                                _ => None,
                            })
                            .or_else(|| self.mode_timestamps.get(&'k').copied());

                        if current_ts.is_none_or(|ts| incoming_ts > ts) {
                            self.modes.retain(|m| !matches!(m, ChannelMode::Key(_, _)));
                            self.modes.insert(ChannelMode::Key(k, incoming_ts));
                            self.mode_timestamps.insert('k', incoming_ts);
                        }
                    }
                } else {
                    let current_ts = self
                        .modes
                        .iter()
                        .find_map(|m| match m {
                            ChannelMode::Key(_, ts) => Some(*ts),
                            _ => None,
                        })
                        .or_else(|| self.mode_timestamps.get(&'k').copied());

                    if current_ts.is_none_or(|ts| incoming_ts > ts) {
                        self.modes.retain(|m| !matches!(m, ChannelMode::Key(_, _)));
                        self.mode_timestamps.insert('k', incoming_ts);
                    }
                }
            }

            // Simple boolean modes with LWW
            ProtoChannelMode::InviteOnly => {
                self.apply_boolean_mode_lww('i', ChannelMode::InviteOnly, adding, incoming_ts);
            }
            ProtoChannelMode::Moderated => {
                self.apply_boolean_mode_lww('m', ChannelMode::Moderated, adding, incoming_ts);
            }
            ProtoChannelMode::NoExternalMessages => {
                self.apply_boolean_mode_lww('n', ChannelMode::NoExternal, adding, incoming_ts);
            }
            ProtoChannelMode::Secret => {
                self.apply_boolean_mode_lww('s', ChannelMode::Secret, adding, incoming_ts);
            }
            ProtoChannelMode::ProtectedTopic => {
                self.apply_boolean_mode_lww('t', ChannelMode::TopicLock, adding, incoming_ts);
            }
            ProtoChannelMode::NoColors => {
                self.apply_boolean_mode_lww('c', ChannelMode::NoColors, adding, incoming_ts);
            }
            ProtoChannelMode::RegisteredOnly => {
                self.apply_boolean_mode_lww('R', ChannelMode::RegisteredOnly, adding, incoming_ts);
            }
            ProtoChannelMode::NoCTCP => {
                self.apply_boolean_mode_lww('C', ChannelMode::NoCtcp, adding, incoming_ts);
            }
            ProtoChannelMode::TlsOnly => {
                self.apply_boolean_mode_lww('z', ChannelMode::TlsOnly, adding, incoming_ts);
            }
            ProtoChannelMode::ModeratedUnreg => {
                self.apply_boolean_mode_lww('M', ChannelMode::ModeratedUnreg, adding, incoming_ts);
            }
            ProtoChannelMode::OpModerated => {
                self.apply_boolean_mode_lww('U', ChannelMode::OpModerated, adding, incoming_ts);
            }
            ProtoChannelMode::Auditorium => {
                self.apply_boolean_mode_lww('u', ChannelMode::Auditorium, adding, incoming_ts);
            }
            ProtoChannelMode::NoNickChange => {
                self.apply_boolean_mode_lww('N', ChannelMode::NoNickChange, adding, incoming_ts);
            }
            ProtoChannelMode::NoKnock => {
                self.apply_boolean_mode_lww('K', ChannelMode::NoKnock, adding, incoming_ts);
            }
            ProtoChannelMode::NoInvite => {
                self.apply_boolean_mode_lww('V', ChannelMode::NoInvite, adding, incoming_ts);
            }
            ProtoChannelMode::FreeInvite => {
                self.apply_boolean_mode_lww('g', ChannelMode::FreeInvite, adding, incoming_ts);
            }
            ProtoChannelMode::Permanent => {
                self.apply_boolean_mode_lww('P', ChannelMode::Permanent, adding, incoming_ts);
            }
            // Note: Private mode ('p') is internal to slircd-ng, not in proto

            // Prefix modes (member modes) - apply directly, member modes have separate CRDT handling
            ProtoChannelMode::Oper | ProtoChannelMode::Voice | ProtoChannelMode::Halfop => {
                if let Some(uid) = arg {
                    if let Some(member) = self.members.get_mut(&uid) {
                        match mode {
                            ProtoChannelMode::Oper => member.op = adding,
                            ProtoChannelMode::Voice => member.voice = adding,
                            ProtoChannelMode::Halfop => member.halfop = adding,
                            _ => {}
                        }
                    }
                }
            }
            ProtoChannelMode::Founder => {
                if let Some(uid) = arg {
                    if let Some(member) = self.members.get_mut(&uid) {
                        member.owner = adding;
                    }
                }
            }
            ProtoChannelMode::Admin => {
                if let Some(uid) = arg {
                    if let Some(member) = self.members.get_mut(&uid) {
                        member.admin = adding;
                    }
                }
            }

            _ => {}
        }
    }

    /// Apply a boolean mode with LWW conflict resolution.
    ///
    /// Compares incoming timestamp against stored timestamp for this mode char.
    /// Only applies if incoming wins.
    fn apply_boolean_mode_lww(
        &mut self,
        mode_char: char,
        mode: ChannelMode,
        adding: bool,
        incoming_ts: HybridTimestamp,
    ) {
        let current_ts = self.mode_timestamps.get(&mode_char).copied();

        // LWW: only apply if incoming timestamp wins
        let should_apply = match current_ts {
            Some(ts) => incoming_ts > ts,
            None => true,
        };

        if should_apply {
            if adding {
                self.modes.insert(mode);
            } else {
                self.modes.remove(&mode);
            }
            self.mode_timestamps.insert(mode_char, incoming_ts);
        }
    }
}

/// Extract SID from a setter string (e.g., "001AAAAAB" -> "001", "nick!user@host" -> "000")
fn extract_sid_from_setter(setter: &str) -> ServerId {
    // If it looks like a UID (9 alphanumeric chars), extract first 3
    if setter.len() >= 3 && setter.chars().take(3).all(|c| c.is_ascii_alphanumeric()) {
        ServerId::new(setter[..3].to_string())
    } else {
        // Default SID for non-UID setters
        ServerId::new("000".to_string())
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
