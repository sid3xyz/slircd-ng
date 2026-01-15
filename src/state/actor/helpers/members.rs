//! Channel member management.
//!
//! Helpers for updating member modes (op, voice, etc.).

use super::super::{ChannelActor, Uid};
use crate::state::MemberModes;
use slirc_proto::sync::clock::{HybridTimestamp, ServerId};

impl ChannelActor {
    pub(crate) fn update_member_mode<F>(&mut self, target_uid: &Uid, mut update: F) -> bool
    where
        F: FnMut(&mut MemberModes),
    {
        if let Some(member) = self.members.get(target_uid).cloned() {
            let mut updated = member.clone();
            update(&mut updated);

            if updated != member {
                // Update timestamps for changed fields
                let sid = if let Some(matrix) = self.matrix.upgrade() {
                    matrix.server_id.clone()
                } else {
                    ServerId::new("000")
                };
                let now = HybridTimestamp::now(&sid);

                if updated.owner != member.owner {
                    updated.owner_ts = Some(now);
                }
                if updated.admin != member.admin {
                    updated.admin_ts = Some(now);
                }
                if updated.op != member.op {
                    updated.op_ts = Some(now);
                }
                if updated.halfop != member.halfop {
                    updated.halfop_ts = Some(now);
                }
                if updated.voice != member.voice {
                    updated.voice_ts = Some(now);
                }

                self.members.insert(target_uid.clone(), updated);
                return true;
            }
        }

        false
    }
}
