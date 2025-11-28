//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.

mod matrix;
mod mode_builder;
mod uid;

pub use matrix::{Channel, ListEntry, Matrix, MemberModes, Topic, User, UserModes};
#[allow(unused_imports)] // Will be used when we implement multi-mode commands
pub use mode_builder::{ChannelModeBuilder, ModeChangeResult};
pub use uid::UidGenerator;
