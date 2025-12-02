//! Channel repository for ChanServ functionality.
//!
//! Handles channel registration, access lists, and settings.

pub mod models;
pub mod queries;

pub use models::{ChannelAkick, ChannelRecord};
pub use queries::ChannelRepository;
