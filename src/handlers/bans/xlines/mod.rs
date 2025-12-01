//! X-line ban command handlers.
//!
//! Server-wide ban commands (operator-only):
//! - KLINE/UNKLINE: Ban/unban by nick!user@host mask
//! - DLINE/UNDLINE: Ban/unban by IP address
//! - GLINE/UNGLINE: Global ban/unban by nick!user@host mask
//! - ZLINE/UNZLINE: Global IP ban/unban (skips DNS)
//! - RLINE/UNRLINE: Ban/unban by realname (GECOS)

pub mod kline;
pub mod dline;
pub mod gline;
pub mod zline;
pub mod rline;

pub use kline::{KlineHandler, UnklineHandler};
pub use dline::{DlineHandler, UndlineHandler};
pub use gline::{GlineHandler, UnglineHandler};
pub use zline::{ZlineHandler, UnzlineHandler};
pub use rline::{RlineHandler, UnrlineHandler};
