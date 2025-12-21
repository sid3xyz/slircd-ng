//! CRDT serialization and merging for ChannelActor.
//!
//! This module handles conversion between the actor's internal state
//! and the CRDT representation used for distributed synchronization.

use crate::state::{ListEntry, MemberModes, Topic};
use slirc_crdt::channel::{ChannelCrdt, ListEntryCrdt, TopicCrdt};
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use std::collections::HashSet;

use super::{ChannelActor, ChannelMode};

impl ChannelActor {
    /// Merge a CRDT representation into the channel state.
    pub async fn handle_merge_crdt(&mut self, crdt: ChannelCrdt, source: Option<ServerId>) {
        use slirc_crdt::traits::Crdt;
        let mut current_crdt = self.to_crdt();
        current_crdt.merge(&crdt);

        // Update self from merged CRDT
        self.apply_merged_topic(&current_crdt);
        self.apply_merged_modes(&current_crdt);
        self.apply_merged_members(&current_crdt).await;
        self.apply_merged_lists(&current_crdt);

        // Notify observer to propagate the change (Split Horizon handled by source)
        self.notify_observer(source);
    }

    /// Apply merged topic from CRDT.
    fn apply_merged_topic(&mut self, crdt: &ChannelCrdt) {
        if let Some(topic_crdt) = crdt.topic.value() {
            self.topic = Some(Topic {
                text: topic_crdt.text.clone(),
                set_by: topic_crdt.set_by.clone(),
                set_at: topic_crdt.set_at,
            });
            self.topic_timestamp = Some(crdt.topic.timestamp());
        } else {
            self.topic = None;
            self.topic_timestamp = None;
        }
    }

    /// Apply merged modes from CRDT.
    fn apply_merged_modes(&mut self, crdt: &ChannelCrdt) {
        let mut new_modes = HashSet::new();

        if *crdt.modes.no_external.value() {
            new_modes.insert(ChannelMode::NoExternal);
            self.mode_timestamps
                .insert('n', crdt.modes.no_external.timestamp());
        }
        if *crdt.modes.topic_ops_only.value() {
            new_modes.insert(ChannelMode::TopicLock);
            self.mode_timestamps
                .insert('t', crdt.modes.topic_ops_only.timestamp());
        }
        if *crdt.modes.moderated.value() {
            new_modes.insert(ChannelMode::Moderated);
            self.mode_timestamps
                .insert('m', crdt.modes.moderated.timestamp());
        }
        if *crdt.modes.invite_only.value() {
            new_modes.insert(ChannelMode::InviteOnly);
            self.mode_timestamps
                .insert('i', crdt.modes.invite_only.timestamp());
        }
        if *crdt.modes.secret.value() {
            new_modes.insert(ChannelMode::Secret);
            self.mode_timestamps
                .insert('s', crdt.modes.secret.timestamp());
        }
        if *crdt.modes.private.value() {
            new_modes.insert(ChannelMode::Private);
            self.mode_timestamps
                .insert('p', crdt.modes.private.timestamp());
        }
        if *crdt.modes.registered_only.value() {
            new_modes.insert(ChannelMode::RegisteredOnly);
            self.mode_timestamps
                .insert('R', crdt.modes.registered_only.timestamp());
        }
        if *crdt.modes.no_colors.value() {
            new_modes.insert(ChannelMode::NoColors);
            self.mode_timestamps
                .insert('c', crdt.modes.no_colors.timestamp());
        }
        if *crdt.modes.no_ctcp.value() {
            new_modes.insert(ChannelMode::NoCtcp);
            self.mode_timestamps
                .insert('C', crdt.modes.no_ctcp.timestamp());
        }
        if *crdt.modes.ssl_only.value() {
            new_modes.insert(ChannelMode::TlsOnly);
            self.mode_timestamps
                .insert('z', crdt.modes.ssl_only.timestamp());
        }

        if let Some(key) = crdt.key.value() {
            new_modes.insert(ChannelMode::Key(key.clone(), crdt.key.timestamp()));
        }
        if let Some(limit) = crdt.limit.value() {
            new_modes.insert(ChannelMode::Limit(*limit as usize, crdt.limit.timestamp()));
        }
        self.modes = new_modes;
    }

    /// Apply merged members from CRDT.
    async fn apply_merged_members(&mut self, crdt: &ChannelCrdt) {
        for uid in crdt.members.iter() {
            if let Some(m_modes_crdt) = crdt.members.get_modes(uid) {
                let m_modes = MemberModes {
                    owner: *m_modes_crdt.owner.value(),
                    owner_ts: Some(m_modes_crdt.owner.timestamp()),
                    admin: *m_modes_crdt.admin.value(),
                    admin_ts: Some(m_modes_crdt.admin.timestamp()),
                    op: *m_modes_crdt.op.value(),
                    op_ts: Some(m_modes_crdt.op.timestamp()),
                    halfop: *m_modes_crdt.halfop.value(),
                    halfop_ts: Some(m_modes_crdt.halfop.timestamp()),
                    voice: *m_modes_crdt.voice.value(),
                    voice_ts: Some(m_modes_crdt.voice.timestamp()),
                    join_time: Some(m_modes_crdt.join_time),
                };
                self.members.insert(uid.clone(), m_modes);

                // Ensure we have a sender for this user
                #[allow(clippy::collapsible_if)]
                if !self.senders.contains_key(uid) {
                    if let Some(matrix) = self.matrix.upgrade() {
                        // Try to get sender from UserManager (local user)
                        if let Some(sender) = matrix.user_manager.senders.get(uid) {
                            self.senders.insert(uid.clone(), sender.clone());
                        } else {
                            // Remote user - use router
                            self.senders.insert(uid.clone(), matrix.router_tx.clone());
                        }
                    }
                }
            }
        }

        // Remove members no longer in CRDT
        let to_remove: Vec<_> = self
            .members
            .keys()
            .filter(|uid| !crdt.members.contains(uid))
            .cloned()
            .collect();
        for uid in to_remove {
            self.members.remove(&uid);
            self.user_nicks.remove(&uid);
            self.senders.remove(&uid);
            self.user_caps.remove(&uid);
        }
    }

    /// Apply merged lists from CRDT.
    fn apply_merged_lists(&mut self, crdt: &ChannelCrdt) {
        self.bans = crdt
            .bans
            .iter()
            .map(|e| ListEntry {
                mask: e.mask.clone(),
                set_by: e.set_by.clone(),
                set_at: e.set_at,
            })
            .collect();
        self.excepts = crdt
            .excepts
            .iter()
            .map(|e| ListEntry {
                mask: e.mask.clone(),
                set_by: e.set_by.clone(),
                set_at: e.set_at,
            })
            .collect();
        self.invex = crdt
            .invites
            .iter()
            .map(|e| ListEntry {
                mask: e.mask.clone(),
                set_by: e.set_by.clone(),
                set_at: e.set_at,
            })
            .collect();
    }

    /// Convert channel state to CRDT representation.
    pub fn to_crdt(&self) -> ChannelCrdt {
        let fallback_ts = self.get_fallback_timestamp();
        let mut crdt = ChannelCrdt::new(self.name.clone(), fallback_ts);

        self.serialize_topic_to_crdt(&mut crdt, fallback_ts);
        self.serialize_modes_to_crdt(&mut crdt, fallback_ts);
        self.serialize_members_to_crdt(&mut crdt, fallback_ts);
        self.serialize_lists_to_crdt(&mut crdt, fallback_ts);

        crdt
    }

    /// Get fallback timestamp for CRDT operations.
    fn get_fallback_timestamp(&self) -> HybridTimestamp {
        if let Some(matrix) = self.matrix.upgrade() {
            HybridTimestamp::now(&matrix.server_id)
        } else {
            HybridTimestamp::now(&ServerId::new("000"))
        }
    }

    /// Serialize topic to CRDT.
    fn serialize_topic_to_crdt(&self, crdt: &mut ChannelCrdt, fallback_ts: HybridTimestamp) {
        if let Some(topic) = &self.topic {
            let topic_ts = self.topic_timestamp.unwrap_or(fallback_ts);
            crdt.topic.update(
                Some(TopicCrdt {
                    text: topic.text.clone(),
                    set_by: topic.set_by.clone(),
                    set_at: topic.set_at,
                }),
                topic_ts,
            );
        }
    }

    /// Serialize modes to CRDT.
    fn serialize_modes_to_crdt(&self, crdt: &mut ChannelCrdt, fallback_ts: HybridTimestamp) {
        for mode in &self.modes {
            match mode {
                ChannelMode::NoExternal => {
                    let ts = self
                        .mode_timestamps
                        .get(&'n')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.no_external.update(true, ts);
                }
                ChannelMode::TopicLock => {
                    let ts = self
                        .mode_timestamps
                        .get(&'t')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.topic_ops_only.update(true, ts);
                }
                ChannelMode::Moderated => {
                    let ts = self
                        .mode_timestamps
                        .get(&'m')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.moderated.update(true, ts);
                }
                ChannelMode::InviteOnly => {
                    let ts = self
                        .mode_timestamps
                        .get(&'i')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.invite_only.update(true, ts);
                }
                ChannelMode::Secret => {
                    let ts = self
                        .mode_timestamps
                        .get(&'s')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.secret.update(true, ts);
                }
                ChannelMode::Private => {
                    let ts = self
                        .mode_timestamps
                        .get(&'p')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.private.update(true, ts);
                }
                ChannelMode::RegisteredOnly => {
                    let ts = self
                        .mode_timestamps
                        .get(&'R')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.registered_only.update(true, ts);
                }
                ChannelMode::NoColors => {
                    let ts = self
                        .mode_timestamps
                        .get(&'c')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.no_colors.update(true, ts);
                }
                ChannelMode::NoCtcp => {
                    let ts = self
                        .mode_timestamps
                        .get(&'C')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.no_ctcp.update(true, ts);
                }
                ChannelMode::TlsOnly => {
                    let ts = self
                        .mode_timestamps
                        .get(&'z')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.ssl_only.update(true, ts);
                }
                ChannelMode::Key(k, ts) => {
                    crdt.key.update(Some(k.clone()), *ts);
                }
                ChannelMode::Limit(l, ts) => {
                    crdt.limit.update(Some(*l as u32), *ts);
                }
                _ => {} // Other modes not yet in CRDT
            }
        }
    }

    /// Serialize members to CRDT.
    fn serialize_members_to_crdt(&self, crdt: &mut ChannelCrdt, fallback_ts: HybridTimestamp) {
        for (uid, modes) in &self.members {
            crdt.members.join(uid.clone(), fallback_ts);
            if let Some(m_crdt) = crdt.members.get_modes_mut(uid) {
                if let Some(ts) = modes.op_ts {
                    m_crdt.op.update(modes.op, ts);
                } else {
                    m_crdt.op.update(modes.op, fallback_ts);
                }
                if let Some(ts) = modes.voice_ts {
                    m_crdt.voice.update(modes.voice, ts);
                } else {
                    m_crdt.voice.update(modes.voice, fallback_ts);
                }
                if let Some(ts) = modes.halfop_ts {
                    m_crdt.halfop.update(modes.halfop, ts);
                } else {
                    m_crdt.halfop.update(modes.halfop, fallback_ts);
                }
                if let Some(ts) = modes.admin_ts {
                    m_crdt.admin.update(modes.admin, ts);
                } else {
                    m_crdt.admin.update(modes.admin, fallback_ts);
                }
                if let Some(ts) = modes.owner_ts {
                    m_crdt.owner.update(modes.owner, ts);
                } else {
                    m_crdt.owner.update(modes.owner, fallback_ts);
                }
            }
        }
    }

    /// Serialize lists to CRDT.
    fn serialize_lists_to_crdt(&self, crdt: &mut ChannelCrdt, fallback_ts: HybridTimestamp) {
        for entry in &self.bans {
            crdt.bans.add(
                ListEntryCrdt {
                    mask: entry.mask.clone(),
                    set_by: entry.set_by.clone(),
                    set_at: entry.set_at,
                },
                fallback_ts,
            );
        }
        for entry in &self.excepts {
            crdt.excepts.add(
                ListEntryCrdt {
                    mask: entry.mask.clone(),
                    set_by: entry.set_by.clone(),
                    set_at: entry.set_at,
                },
                fallback_ts,
            );
        }
        for entry in &self.invex {
            crdt.invites.add(
                ListEntryCrdt {
                    mask: entry.mask.clone(),
                    set_by: entry.set_by.clone(),
                    set_at: entry.set_at,
                },
                fallback_ts,
            );
        }
    }

    /// Notify the observer of a state change.
    pub fn notify_observer(&self, source: Option<ServerId>) {
        if let Some(observer) = &self.observer {
            let crdt = self.to_crdt();
            observer.on_channel_update(&crdt, source);
        }
    }
}
