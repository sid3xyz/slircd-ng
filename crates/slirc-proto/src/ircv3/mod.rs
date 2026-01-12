//! IRCv3 extensions and utilities.
//!
//! This module provides helpers for IRCv3 features including:
//! - Batch reference generation
//! - Message ID generation
//! - Server-time formatting

/// Batch processing utilities.
pub mod batch;
/// Message ID generation.
pub mod msgid;
/// Server-time formatting.
pub mod server_time;

pub use self::batch::generate_batch_ref;
pub use self::msgid::generate_msgid;
pub use self::server_time::{format_server_time, format_timestamp, parse_server_time};
