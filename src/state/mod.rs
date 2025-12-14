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
mod matrix;
mod mode_builder;
pub mod session;
mod uid;
mod user;

pub use channel::{ListEntry, MemberModes, Topic};
pub use matrix::Matrix;
pub use user::WhowasEntry;
pub mod actor;
pub use user::{User, UserModes};

// Session state types (Innovation 1 Phase 3: True Typestate)
pub use session::{RegisteredState, SessionState, UnregisteredState};

// Internal re-exports
pub(crate) use uid::UidGenerator;
