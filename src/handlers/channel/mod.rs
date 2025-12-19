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
pub use ops::{TargetUser, force_join_channel, force_part_channel};
pub use part::PartHandler;
pub use topic::TopicHandler;

use std::collections::HashMap;
use crate::handlers::PostRegHandler;

pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("CYCLE", Box::new(CycleHandler));
    map.insert("INVITE", Box::new(InviteHandler));
    map.insert("JOIN", Box::new(JoinHandler));
    map.insert("KICK", Box::new(KickHandler));
    map.insert("KNOCK", Box::new(KnockHandler));
    map.insert("LIST", Box::new(ListHandler));
    map.insert("NAMES", Box::new(NamesHandler));
    map.insert("PART", Box::new(PartHandler));
    map.insert("TOPIC", Box::new(TopicHandler));
}
