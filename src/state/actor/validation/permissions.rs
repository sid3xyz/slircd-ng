use super::super::{ChannelActor, Uid};

impl ChannelActor {
    pub(crate) fn member_has_voice_or_higher(&self, uid: &Uid) -> bool {
        self.members
            .get(uid)
            .map(|m| m.has_voice_or_higher())
            .unwrap_or(false)
    }

    pub(crate) fn member_has_halfop_or_higher(&self, uid: &Uid) -> bool {
        self.members
            .get(uid)
            .map(|m| m.has_halfop_or_higher())
            .unwrap_or(false)
    }
}
