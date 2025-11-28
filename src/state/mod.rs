//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.

mod matrix;
mod uid;

pub use matrix::{Channel, ListEntry, Matrix, MemberModes, Topic, User, UserModes};
pub use uid::UidGenerator;
