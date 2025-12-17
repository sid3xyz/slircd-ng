//! Channel validation helpers for the actor model.
//!
//! Provides permission checking, ban matching, and invite management
//! utilities used by [`ChannelActor`](super::ChannelActor) handlers.

pub mod bans;
pub mod invites;
pub mod permissions;

pub use bans::{create_user_mask, format_user_mask, is_banned};
