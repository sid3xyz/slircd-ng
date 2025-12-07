//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.
//!
//! ## Protocol State Machine (Innovation 1 Phase 3)
//!
//! The `session` module provides true typestate types for compile-time
//! enforcement of protocol state transitions. State types hold actual data,
//! not just markers. See [`session`] for details.

mod channel;
mod machine;
mod matrix;
mod mode_builder;
pub mod session;
mod uid;
mod user;

pub use channel::{ListEntry, MemberModes, Topic};
pub use matrix::Matrix;
pub mod actor;
pub use user::{User, UserModes};

// Session state types (Innovation 1 Phase 3: True Typestate)
// Allow unused while migrating - these will replace HandshakeState
#[allow(unused_imports)]
pub use session::{ConnectionState, RegisteredState, UnregisteredState};

// Legacy protocol state machine types (to be removed after Phase 3 migration)
#[allow(unused_imports)]
pub use machine::{
    AnyConnectionState as LegacyAnyConnectionState,
    CanNegotiate,
    ConnectionState as LegacyConnectionState,
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
