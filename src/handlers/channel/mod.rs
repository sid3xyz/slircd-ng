//! Channel command handlers.
//!
//! Handles JOIN, PART, TOPIC, NAMES, KICK, INVITE, KNOCK, CYCLE commands.

mod cycle;
mod invite;
mod join;
mod kick;
mod knock;
mod list;
mod names;
mod ops;
mod part;
mod topic;

pub use cycle::CycleHandler;
pub use invite::InviteHandler;
pub use join::JoinHandler;
pub use kick::KickHandler;
pub use knock::KnockHandler;
pub use list::ListHandler;
pub use names::NamesHandler;
pub use ops::{force_join_channel, force_part_channel, TargetUser};
pub use part::PartHandler;
pub use topic::TopicHandler;

use crate::security::UserContext;
use crate::state::ListEntry;

/// Check if a ban entry matches a user, supporting both hostmask and extended bans.
pub(super) fn matches_ban(entry: &ListEntry, user_mask: &str, user_context: &UserContext) -> bool {
    super::matches_ban_or_except(&entry.mask, user_mask, user_context)
}
