use super::super::{ChannelActor, INVITE_TTL, InviteEntry, MAX_INVITES_PER_CHANNEL, Uid};
use std::time::Instant;

impl ChannelActor {
    pub(crate) fn prune_invites(&mut self) {
        while let Some(front) = self.invites.front() {
            if front.set_at.elapsed() > INVITE_TTL {
                self.invites.pop_front();
            } else {
                break;
            }
        }
    }

    pub(crate) fn add_invite(&mut self, uid: Uid) {
        self.prune_invites();

        if self.invites.iter().any(|entry| entry.uid == uid) {
            return;
        }

        self.invites.push_back(InviteEntry {
            uid,
            set_at: Instant::now(),
        });

        while self.invites.len() > MAX_INVITES_PER_CHANNEL {
            self.invites.pop_front();
        }
    }

    pub(crate) fn remove_invite(&mut self, uid: &Uid) {
        self.invites.retain(|entry| &entry.uid != uid);
    }

    pub(crate) fn is_invited(&mut self, uid: &Uid) -> bool {
        self.prune_invites();
        self.invites.iter().any(|entry| &entry.uid == uid)
    }
}
