//! Domain managers for server state.
//!
//! This module contains specialized managers that each own a specific domain
//! of the IRC server's state. This separation reduces coupling and makes
//! the codebase easier to maintain and test.

pub mod channel;
pub mod client;
pub mod lifecycle;
pub mod monitor;
pub mod security;
pub mod service;
pub mod stats;
pub mod user;
