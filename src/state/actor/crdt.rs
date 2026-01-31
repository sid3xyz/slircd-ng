//! CRDT serialization and merging for ChannelActor.
//!
//! This module handles conversion between the actor's internal state
//! and the CRDT representation used for distributed synchronization.

use crate::state::{ListEntry, MemberModes, Topic};
use slirc_proto::sync::channel::{ChannelCrdt, ListEntryCrdt, TopicCrdt};
use slirc_proto::sync::clock::{HybridTimestamp, ServerId};
use std::collections::HashSet;

use super::{ChannelActor, ChannelMode};

impl ChannelActor {
    /// Merge a CRDT representation into the channel state.
    pub async fn handle_merge_crdt(&mut self, crdt: ChannelCrdt, source: Option<ServerId>) {
        use slirc_proto::sync::traits::Crdt;
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
        if *crdt.modes.delayed_join.value() {
            new_modes.insert(ChannelMode::DelayedJoin);
            self.mode_timestamps
                .insert('D', crdt.modes.delayed_join.timestamp());
        }
        if *crdt.modes.strip_colors.value() {
            new_modes.insert(ChannelMode::StripColors);
            self.mode_timestamps
                .insert('S', crdt.modes.strip_colors.timestamp());
        }
        if *crdt.modes.anti_caps.value() {
            new_modes.insert(ChannelMode::AntiCaps);
            self.mode_timestamps
                .insert('B', crdt.modes.anti_caps.timestamp());
        }
        if *crdt.modes.censor.value() {
            new_modes.insert(ChannelMode::Censor);
            self.mode_timestamps
                .insert('G', crdt.modes.censor.timestamp());
        }

        if let Some(key) = crdt.key.value() {
            new_modes.insert(ChannelMode::Key(key.clone(), crdt.key.timestamp()));
        }
        if let Some(limit) = crdt.limit.value() {
            new_modes.insert(ChannelMode::Limit(*limit as usize, crdt.limit.timestamp()));
        }
        if let Some(redirect) = crdt.modes.redirect.value() {
            new_modes.insert(ChannelMode::Redirect(
                redirect.clone(),
                crdt.modes.redirect.timestamp(),
            ));
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
                        if let Some(sender) = matrix.user_manager.get_first_sender(uid) {
                            self.senders.insert(uid.clone(), sender);
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
        // Initialize CRDT with zero timestamp to ensure it accepts all historical state
        // from the actor. If we handled it with fallback_ts (Now), it would reject
        // any state older than Now due to LWW rules.
        let base_ts = HybridTimestamp::new(0, 0, &ServerId::new("000"));
        let mut crdt = ChannelCrdt::new(self.name.clone(), base_ts);

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
                ChannelMode::DelayedJoin => {
                    let ts = self
                        .mode_timestamps
                        .get(&'D')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.delayed_join.update(true, ts);
                }
                ChannelMode::StripColors => {
                    let ts = self
                        .mode_timestamps
                        .get(&'S')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.strip_colors.update(true, ts);
                }
                ChannelMode::AntiCaps => {
                    let ts = self
                        .mode_timestamps
                        .get(&'B')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.anti_caps.update(true, ts);
                }
                ChannelMode::Censor => {
                    let ts = self
                        .mode_timestamps
                        .get(&'G')
                        .copied()
                        .unwrap_or(fallback_ts);
                    crdt.modes.censor.update(true, ts);
                }
                ChannelMode::Key(k, ts) => {
                    crdt.key.update(Some(k.clone()), *ts);
                }
                ChannelMode::Limit(l, ts) => {
                    crdt.limit.update(Some(*l as u32), *ts);
                }
                ChannelMode::Redirect(target, ts) => {
                    crdt.modes.redirect.update(Some(target.clone()), *ts);
                }
                _ => {} // Other modes not yet in CRDT
            }
        }
    }

    /// Serialize members to CRDT.
    fn serialize_members_to_crdt(&self, crdt: &mut ChannelCrdt, fallback_ts: HybridTimestamp) {
        let base_ts = HybridTimestamp::new(0, 0, &ServerId::new("000"));
        for (uid, modes) in &self.members {
            // Join at real join_time (or fallback to base_ts if missing) to preserve causality
            let join_ts = if let Some(join_time) = modes.join_time {
                // We use ServerId 000 because join_time is a scalar (Unix timestamp)
                // and we don't have the original SID of the join easily accessible here.
                // This is a simplified approximation but better than TS=0.
                HybridTimestamp::new(join_time as u64 * 1000, 0, &ServerId::new("000"))
            } else {
                base_ts
            };
            crdt.members.join(uid.clone(), join_ts);

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
#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::MemberModes;
    use crate::state::actor::{ChannelActor, ChannelMode};

    // Removed unused imports: Crdt trait, HashMap, HashSet

    fn make_actor(name: &str) -> ChannelActor {
        ChannelActor::new_test(name.to_string(), ServerId::new("000"))
    }

    #[test]
    fn test_serialization_topic() {
        let mut actor = make_actor("#test");
        let t0 = HybridTimestamp::new(100, 0, &ServerId::new("000"));

        actor.topic = Some(Topic {
            text: "Hello World".to_string(),
            set_by: "Nick!User@Host".to_string(),
            set_at: 1234567890,
        });
        actor.topic_timestamp = Some(t0);

        let crdt = actor.to_crdt();
        assert_eq!(crdt.name, "#test");

        let topic_c = crdt.topic.value().as_ref().unwrap();
        assert_eq!(topic_c.text, "Hello World");
        assert_eq!(topic_c.set_by, "Nick!User@Host");
        assert_eq!(crdt.topic.timestamp(), t0);
    }

    #[test]
    fn test_serialization_modes() {
        let mut actor = make_actor("#test");
        let t1 = HybridTimestamp::new(101, 0, &ServerId::new("001"));
        let t2 = HybridTimestamp::new(102, 0, &ServerId::new("002"));

        actor.modes.insert(ChannelMode::NoExternal);
        actor.mode_timestamps.insert('n', t1);

        actor.modes.insert(ChannelMode::Key("secret".into(), t2));

        let crdt = actor.to_crdt();

        // Check boolean mode
        assert!(crdt.modes.no_external.value());
        assert_eq!(crdt.modes.no_external.timestamp(), t1);

        // Check parameter mode
        assert_eq!(crdt.key.value().as_deref(), Some("secret"));
        assert_eq!(crdt.key.timestamp(), t2);
    }

    #[test]
    fn test_serialization_members() {
        let mut actor = make_actor("#test");
        let t1 = HybridTimestamp::new(101, 0, &ServerId::new("001"));
        let t2 = HybridTimestamp::new(102, 0, &ServerId::new("001"));

        let mut modes = MemberModes::default();
        modes.op = true;
        modes.op_ts = Some(t1);
        modes.voice = true;
        modes.voice_ts = Some(t2);

        actor.members.insert("user1".to_string(), modes);

        let crdt = actor.to_crdt();

        assert!(crdt.members.contains("user1"));
        let m_crdt = crdt.members.get_modes("user1").unwrap();

        assert!(m_crdt.op.value());
        assert_eq!(m_crdt.op.timestamp(), t1);
        assert!(m_crdt.voice.value());
        assert_eq!(m_crdt.voice.timestamp(), t2);
    }

    #[tokio::test]
    async fn test_apply_merge() {
        let mut actor = make_actor("#test");
        let sid = ServerId::new("00B");
        let t0 = HybridTimestamp::new(100, 0, &sid);
        let t1 = HybridTimestamp::new(200, 0, &sid);

        // Create an incoming CRDT state
        let mut crdt = ChannelCrdt::new("#test".to_string(), t0);
        crdt.topic.update(
            Some(TopicCrdt {
                text: "New Topic".to_string(),
                set_by: "Remote".to_string(),
                set_at: 2000000000,
            }),
            t1,
        );
        crdt.modes.moderated.update(true, t1);

        // Merge it
        actor.handle_merge_crdt(crdt, None).await;

        // Verify actor state updated
        assert_eq!(actor.topic.as_ref().unwrap().text, "New Topic");
        assert_eq!(actor.topic_timestamp, Some(t1));

        assert!(actor.modes.contains(&ChannelMode::Moderated));
        assert_eq!(*actor.mode_timestamps.get(&'m').unwrap(), t1);
    }
}
