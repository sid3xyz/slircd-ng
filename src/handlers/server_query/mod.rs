//! Server query handlers: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, USERIP, LINKS, HELP
//!
//! RFC 2812 §3.4 - Server queries and commands
//! RFC 2812 §3.5 - Service queries (SERVLIST, SQUERY)

mod help;
mod info;
mod motd;
mod service;
mod stats;

pub use help::HelpHandler;
pub use info::{LinksHandler, MapHandler, RulesHandler, UseripHandler};
pub use motd::{AdminHandler, InfoHandler, ListHandler, LusersHandler, MotdHandler, TimeHandler, VersionHandler};
pub use service::{ServiceHandler, ServlistHandler, SqueryHandler};
pub use stats::StatsHandler;
