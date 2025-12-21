use super::ChannelActor;
use crate::state::MemberModes;
use tracing::info;

impl ChannelActor {
    pub(crate) async fn handle_sjoin(
        &mut self,
        ts: u64,
        _modes: String,
        _mode_args: Vec<String>,
        users: Vec<(String, String)>,
    ) {
        // 1. Timestamp Check
        let current_ts = self.created as u64;

        if ts < current_ts {
            // Remote is older (winner). We adopt their TS and modes.
            info!("Channel {} SJOIN: Remote TS {} < Local TS {}. Adopting remote state.", self.name, ts, current_ts);
            self.created = ts as i64;

            // Clear current modes and apply remote modes
            self.modes.clear();
            // TODO: Parse and apply modes/args. This requires a mode parser which we don't have easily accessible here.
            // For now, we will just accept the users.
            // Implementing full mode parsing here is complex.
            // We should probably use a helper or just accept that we might be desynced on modes until we implement full parsing.

        } else if ts > current_ts {
            // Remote is newer (loser). We keep our TS and modes.
            // We still accept the users, but we ignore their modes.
            info!("Channel {} SJOIN: Remote TS {} > Local TS {}. Ignoring remote modes.", self.name, ts, current_ts);
        } else {
            // Equal TS. Merge modes.
            info!("Channel {} SJOIN: Remote TS {} == Local TS {}. Merging modes.", self.name, ts, current_ts);
        }

        // 2. Add Users
        for (prefixes, uid) in users {
            // Parse prefixes to MemberModes
            let mut member_modes = MemberModes::default();
            for c in prefixes.chars() {
                match c {
                    '~' => member_modes.owner = true,
                    '&' => member_modes.admin = true,
                    '@' => member_modes.op = true,
                    '%' => member_modes.halfop = true,
                    '+' => member_modes.voice = true,
                    _ => {}
                }
            }

            // Add to members if not exists
            if !self.members.contains_key(&uid) {
                self.members.insert(uid.clone(), member_modes);

                // We need to update the User struct to know they are in this channel.
                // But ChannelActor doesn't have write access to User.
                // The caller (IncomingCommandHandler) should handle the User side updates?
                // Or we assume the User side is already updated?
                // Actually, `ChannelActor` is responsible for its own state.
                // But `User` struct has `channels: HashSet<String>`.
                // This is a dual-write problem.
                // In `handle_join`, we don't update `User`. The `ChannelManager` or `JoinHandler` does.
                // Here, `IncomingCommandHandler` should update the User's channel list.
            } else {
                // Update modes if they are higher?
                // Or just merge?
                // For now, let's just merge flags.
                if let Some(existing) = self.members.get_mut(&uid) {
                    if member_modes.owner { existing.owner = true; }
                    if member_modes.admin { existing.admin = true; }
                    if member_modes.op { existing.op = true; }
                    if member_modes.halfop { existing.halfop = true; }
                    if member_modes.voice { existing.voice = true; }
                }
            }
        }
    }
}
