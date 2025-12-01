//! Ban command handlers.
//!
//! Commands for server bans (operator-only):
//! - KLINE/UNKLINE: Ban by nick!user@host mask
//! - DLINE/UNDLINE: Ban by IP address
//! - GLINE/UNGLINE: Global ban by nick!user@host mask
//! - ZLINE/UNZLINE: Global IP ban (skips DNS)
//! - RLINE/UNRLINE: Ban by realname (GECOS)
//! - SHUN/UNSHUN: Silently ignore commands from matching users

mod common;
mod shun;
mod xlines;

// Re-export handlers
pub use shun::{ShunHandler, UnshunHandler};
pub use xlines::{
    DlineHandler, GlineHandler, KlineHandler, RlineHandler, UndlineHandler, UnglineHandler,
    UnklineHandler, UnrlineHandler, UnzlineHandler, ZlineHandler,
};
