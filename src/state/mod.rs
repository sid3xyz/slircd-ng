//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.

mod matrix;
mod mode_builder;
mod uid;

pub use matrix::{Channel, ChannelModes, ListEntry, Matrix, MemberModes, Topic, User, UserModes};
pub use mode_builder::{ChannelModeBuilder, ModeChangeResult};
pub use uid::UidGenerator;
