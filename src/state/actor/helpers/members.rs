use super::super::{ChannelActor, Uid};
use crate::state::MemberModes;

impl ChannelActor {
    pub(crate) fn update_member_mode<F>(&mut self, target_uid: &Uid, mut update: F) -> bool
    where
        F: FnMut(&mut MemberModes),
    {
        if let Some(member) = self.members.get(target_uid).cloned() {
            let mut updated = member.clone();
            update(&mut updated);

            if updated != member {
                self.members.insert(target_uid.clone(), updated);
                return true;
            }
        }

        false
    }
}
