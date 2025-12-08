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
//!
//! ## Typestate Handler System (Innovation 1 Phase 3)
//!
//! The handler system enforces protocol state at compile time:
//!
//! - [`PreRegHandler`]: Commands valid before registration (NICK, USER, CAP, etc.)
//! - [`PostRegHandler`]: Commands requiring registration (PRIVMSG, JOIN, etc.)
//! - [`UniversalHandler<S>`]: Commands valid in any state (QUIT, PING, PONG)
//! - [`DynUniversalHandler`]: Object-safe trait for registry storage
//!
//! This eliminates runtime `if !registered` checks by making invalid dispatch
//! a compile-time error. See [`core::traits`] for details.

mod account;
mod admin;
mod bans;
pub mod batch;
pub mod cap;
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
    Context, HandlerError, HandlerResult, Registry, ResponseMiddleware,
    get_nick_or_star, get_oper_info, is_user_in_channel, resolve_nick_to_uid,
    user_mask_from_state,
};

// Re-export typestate handler traits (Innovation 1 Phase 3)
pub use core::{PostRegHandler, PreRegHandler, UniversalHandler};

// Re-export helper functions for use by handlers
pub use helpers::{
    err_chanoprivsneeded, err_needmoreparams, err_noprivileges, err_nosuchchannel, err_nosuchnick,
    err_notonchannel, err_usernotinchannel, labeled_ack, matches_hostmask, server_notice,
    server_reply, user_prefix, with_label,
};

// Re-export types used by other modules
pub use batch::{process_batch_message, BatchState};
pub use cap::SaslState;
pub use channel::{TargetUser, force_join_channel, force_part_channel};
pub use mode::format_modes_for_log;
pub use monitor::{cleanup_monitors, notify_monitors_offline, notify_monitors_online};
