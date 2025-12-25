//! Channel actor event handlers.
//!
//! Each submodule handles a category of [`ChannelEvent`](super::ChannelEvent)
//! messages processed by [`ChannelActor`](super::ChannelActor).

use super::*;

pub mod broadcast;
pub mod invite_knock;
pub mod join;
pub mod kick;
pub mod message;
pub mod modes;
pub mod part_quit;
pub mod remote;
pub mod topic;
