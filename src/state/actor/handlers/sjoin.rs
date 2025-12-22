use super::ChannelActor;
use crate::state::MemberModes;
use crate::state::actor::ChannelMode as ActorMode;
use slirc_crdt::clock::HybridTimestamp;
use slirc_proto::{ChannelMode as ProtoMode, Mode};
use tracing::{info, warn};

impl ChannelActor {
    pub(crate) async fn handle_sjoin(
        &mut self,
        ts: u64,
        modes: String,
        mode_args: Vec<String>,
        users: Vec<(String, String)>,
    ) {
        // 1. Timestamp Check
        let current_ts = self.created as u64;

        if ts < current_ts {
            // Remote is older (winner). We adopt their TS and modes.
            info!(
                "Channel {} SJOIN: Remote TS {} < Local TS {}. Adopting remote state.",
                self.name, ts, current_ts
            );
            self.created = ts as i64;

            // Clear current modes and apply remote modes
            self.modes.clear();

            // Construct args for parser
            let mut args = vec![modes.as_str()];
            for arg in &mode_args {
                args.push(arg.as_str());
            }

            match Mode::as_channel_modes(&args) {
                Ok(parsed_modes) => {
                    for mode in parsed_modes {
                        if let Mode::Plus(m, arg) = mode {
                            let hts = HybridTimestamp::new((ts as i64) * 1000, 0, &self.server_id);
                            let actor_mode = match m {
                                ProtoMode::NoExternalMessages => Some(ActorMode::NoExternal),
                                ProtoMode::ProtectedTopic => Some(ActorMode::TopicLock),
                                ProtoMode::Moderated => Some(ActorMode::Moderated),
                                ProtoMode::ModeratedUnreg => Some(ActorMode::ModeratedUnreg),
                                ProtoMode::OpModerated => Some(ActorMode::OpModerated),
                                ProtoMode::NoNickChange => Some(ActorMode::NoNickChange),
                                ProtoMode::NoColors => Some(ActorMode::NoColors),
                                ProtoMode::TlsOnly => Some(ActorMode::TlsOnly),
                                ProtoMode::NoKnock => Some(ActorMode::NoKnock),
                                ProtoMode::NoInvite => Some(ActorMode::NoInvite),
                                ProtoMode::NoChannelNotice => Some(ActorMode::NoNotice),
                                ProtoMode::FreeInvite => Some(ActorMode::FreeInvite),
                                ProtoMode::OperOnly => Some(ActorMode::OperOnly),
                                ProtoMode::Auditorium => Some(ActorMode::Auditorium),
                                ProtoMode::RegisteredOnly => Some(ActorMode::RegisteredOnly),
                                ProtoMode::NoKick => Some(ActorMode::NoKicks),
                                ProtoMode::Secret => Some(ActorMode::Secret),
                                ProtoMode::InviteOnly => Some(ActorMode::InviteOnly),
                                ProtoMode::NoCTCP => Some(ActorMode::NoCtcp),
                                ProtoMode::Permanent => Some(ActorMode::Permanent),
                                ProtoMode::Key => arg.map(|k| ActorMode::Key(k, hts)),
                                ProtoMode::Limit => {
                                    arg.and_then(|s| s.parse().ok()).map(|l| ActorMode::Limit(l, hts))
                                }
                                _ => None,
                            };

                            if let Some(am) = actor_mode {
                                self.modes.insert(am);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse modes in SJOIN for {}: {} ({})",
                        self.name, modes, e
                    );
                }
            }
        } else if ts > current_ts {
            // Remote is newer (loser). We keep our TS and modes.
            // We still accept the users, but we ignore their modes.
            info!(
                "Channel {} SJOIN: Remote TS {} > Local TS {}. Ignoring remote modes.",
                self.name, ts, current_ts
            );
        } else {
            // Equal TS. Merge modes.
            info!(
                "Channel {} SJOIN: Remote TS {} == Local TS {}. Merging modes.",
                self.name, ts, current_ts
            );
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
                    if member_modes.owner {
                        existing.owner = true;
                    }
                    if member_modes.admin {
                        existing.admin = true;
                    }
                    if member_modes.op {
                        existing.op = true;
                    }
                    if member_modes.halfop {
                        existing.halfop = true;
                    }
                    if member_modes.voice {
                        existing.voice = true;
                    }
                }
            }
        }
    }
}
