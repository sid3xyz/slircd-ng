//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.
//!
//! ## Protocol State Machine (Innovation 1)
//!
//! The `machine` submodule provides typestate types for compile-time
//! enforcement of protocol state transitions. See [`machine`] for details.

mod channel;
mod machine;
mod matrix;
mod mode_builder;
mod uid;
mod user;

pub use channel::{ListEntry, MemberModes, Topic};
pub use matrix::Matrix;
pub mod actor;
pub use user::{User, UserModes};

// Protocol state machine types (Innovation 1: Typestate)
// Will be used in Phase 2+ of handler migration
#[allow(unused_imports)]
pub use machine::{
    AnyConnectionState,
    CanNegotiate,
    ConnectionState,
    IsRegistered,
    Negotiating,
    POST_REG_COMMANDS,
    PRE_REG_COMMANDS,
    PreRegistration,
    ProtocolState,
    Registered,
    Unregistered,
    // Classification helpers
    requires_registration,
    valid_pre_registration,
};
// Exports used by matrix.rs internally

#[allow(unused_imports)]
pub(crate) use user::WhowasEntry;
// Uid is used in security/rate_limit.rs - allow for now
#[allow(unused_imports)]
pub use matrix::Uid;
#[allow(unused_imports)] // Will be used when we implement multi-mode commands
pub use mode_builder::{ChannelModeBuilder, ModeChangeResult, parse_mlock};
pub use uid::UidGenerator;
