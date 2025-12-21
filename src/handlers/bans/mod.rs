//! Ban command handlers.
//!
//! Commands for server bans (operator-only):
//! - KLINE/UNKLINE: Ban by nick!user@host mask
//! - DLINE/UNDLINE: Ban by IP address
//! - GLINE/UNGLINE: Global ban by nick!user@host mask
//! - ZLINE/UNZLINE: Global IP ban (skips DNS)
//! - RLINE/UNRLINE: Ban by realname (GECOS)
//! - SHUN/UNSHUN: Silently ignore commands from matching users

use crate::handlers::PostRegHandler;
use std::collections::HashMap;

mod common;
mod shun;
mod xlines;

// Re-export handlers
pub use shun::{ShunHandler, UnshunHandler};
pub use xlines::{
    DlineHandler, GlineHandler, KlineHandler, RlineHandler, UndlineHandler, UnglineHandler,
    UnklineHandler, UnrlineHandler, UnzlineHandler, ZlineHandler,
};

pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("KLINE", Box::new(KlineHandler::kline()));
    map.insert("UNKLINE", Box::new(UnklineHandler::unkline()));
    map.insert("DLINE", Box::new(DlineHandler::dline()));
    map.insert("UNDLINE", Box::new(UndlineHandler::undline()));
    map.insert("GLINE", Box::new(GlineHandler::gline()));
    map.insert("UNGLINE", Box::new(UnglineHandler::ungline()));
    map.insert("ZLINE", Box::new(ZlineHandler::zline()));
    map.insert("UNZLINE", Box::new(UnzlineHandler::unzline()));
    map.insert("RLINE", Box::new(RlineHandler::rline()));
    map.insert("UNRLINE", Box::new(UnrlineHandler::unrline()));
    map.insert("SHUN", Box::new(ShunHandler));
    map.insert("UNSHUN", Box::new(UnshunHandler));
}
