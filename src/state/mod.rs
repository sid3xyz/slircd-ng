//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.

mod channel;
mod matrix;
mod mode_builder;
mod uid;
mod user;

pub use channel::{Channel, ListEntry, MemberModes, Topic};
pub use matrix::Matrix;
pub use user::{User, UserModes};
// Exports used by matrix.rs internally
#[allow(unused_imports)]
pub(crate) use channel::ChannelModes;
#[allow(unused_imports)]
pub(crate) use user::WhowasEntry;
// Uid is used in security/rate_limit.rs - allow for now
#[allow(unused_imports)]
pub use matrix::Uid;
#[allow(unused_imports)] // Will be used when we implement multi-mode commands
pub use mode_builder::{ChannelModeBuilder, ModeChangeResult, parse_mlock};
pub use uid::UidGenerator;
