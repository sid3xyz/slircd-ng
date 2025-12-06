//! IRC command handlers.
//!
//! This module contains the Handler trait and command registry for dispatching
//! incoming IRC messages to appropriate handlers.
//!
//! ## Zero-Copy Architecture
//!
//! Handlers receive `MessageRef<'_>` which borrows directly from the transport
//! buffer, avoiding allocations in the hot loop. Use `msg.arg(n)` to access
//! arguments as `&str` slices.

mod account;
mod admin;
mod bans;
mod batch;
mod cap;
mod channel;
mod chathistory;
mod connection;
mod core;
mod helpers;
mod messaging;
mod mode;
mod monitor;
mod oper;
mod server_query;
mod service_aliases;
mod user_query;
mod user_status;

// Re-export core types
pub use core::{
    Context, Handler, HandlerError, HandlerResult, HandshakeState, Registry, ResponseMiddleware,
    get_nick_or_star, get_oper_info, require_oper, require_registered, resolve_nick_to_uid,
    user_mask_from_state,
};

// Re-export helper functions for use by handlers
pub use helpers::{
    err_chanoprivsneeded, err_needmoreparams, err_noprivileges, err_nosuchchannel, err_nosuchnick,
    err_notonchannel, err_notregistered, err_usernotinchannel, labeled_ack, matches_hostmask,
    server_notice, server_reply, user_prefix, with_label,
};

// Re-export types used by other modules
pub use batch::process_batch_message;
pub use channel::{TargetUser, force_join_channel, force_part_channel};
pub use mode::format_modes_for_log;
pub use monitor::{cleanup_monitors, notify_monitors_offline, notify_monitors_online};

