//! State management module.
//!
//! Contains the Matrix (shared server state) and related entities.

mod matrix;
mod uid;

pub use matrix::{Channel, Matrix, MemberModes, Topic, User};
pub use uid::UidGenerator;
