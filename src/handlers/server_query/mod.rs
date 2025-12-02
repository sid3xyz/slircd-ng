//! Server query handlers: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, USERIP, LINKS
//!
//! RFC 2812 §3.4 - Server queries and commands

mod info;
mod motd;
mod stats;

pub use info::{LinksHandler, MapHandler, RulesHandler, UseripHandler};
pub use motd::{AdminHandler, InfoHandler, ListHandler, LusersHandler, MotdHandler, TimeHandler, VersionHandler};
pub use stats::StatsHandler;
