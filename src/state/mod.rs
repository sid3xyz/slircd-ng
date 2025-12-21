//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.
//!
//! ## Protocol State Machine (Innovation 1)
//!
//! The `session` module provides typestate types for compile-time
//! enforcement of protocol state transitions. State types hold actual data,
//! not just markers. See [`session`] for details.

mod channel;
pub mod managers;
mod matrix;
pub mod observer;
pub mod session;
mod uid;
mod user;

pub use crate::sync::SyncManager;
pub use channel::{ListEntry, MemberModes, Topic};
pub use managers::channel::ChannelManager;
pub use managers::lifecycle::LifecycleManager;
pub use managers::monitor::MonitorManager;
pub use managers::security::{SecurityManager, SecurityManagerParams};
pub use managers::service::ServiceManager;
pub use managers::user::UserManager;
pub use matrix::{Matrix, MatrixParams};
pub use user::WhowasEntry;
pub mod actor;
pub use user::{User, UserModes, UserParams};

// Session state types (Innovation 1: Typestate pattern)
pub use session::{
    BatchRouting, InitiatorData, RegisteredState, ServerState, SessionState, UnregisteredState,
};

// Internal re-exports
pub use uid::Uid;
pub(crate) use uid::UidGenerator;
