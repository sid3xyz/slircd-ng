//! Server query handlers: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, USERIP, LINKS, HELP
//!
//! RFC 2812 §3.4 - Server queries and commands
//! RFC 2812 §3.5 - Service queries (SERVLIST, SQUERY)

mod disabled;
mod help;
mod info;
mod lusers;
mod server_info;
mod service;
mod stats;

pub use disabled::{SummonHandler, UsersHandler};
pub use help::HelpHandler;
pub use info::{LinksHandler, MapHandler, RulesHandler, UseripHandler};
pub use lusers::LusersHandler;
pub use server_info::{
    AdminHandler, InfoHandler, MotdHandler, TimeHandler, VersionHandler,
};
pub use service::{ServiceHandler, ServlistHandler, SqueryHandler};
pub use stats::StatsHandler;

use crate::handlers::PostRegHandler;
use std::collections::HashMap;

pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("HELP", Box::new(HelpHandler));
    map.insert("LINKS", Box::new(LinksHandler));
    map.insert("MAP", Box::new(MapHandler));
    map.insert("RULES", Box::new(RulesHandler));
    map.insert("USERIP", Box::new(UseripHandler));
    map.insert("ADMIN", Box::new(AdminHandler));
    map.insert("INFO", Box::new(InfoHandler));
    map.insert("LUSERS", Box::new(LusersHandler));
    map.insert("MOTD", Box::new(MotdHandler));
    map.insert("TIME", Box::new(TimeHandler));
    map.insert("VERSION", Box::new(VersionHandler));
    map.insert("SERVICE", Box::new(ServiceHandler));
    map.insert("SERVLIST", Box::new(ServlistHandler));
    map.insert("SQUERY", Box::new(SqueryHandler));
    map.insert("STATS", Box::new(StatsHandler));
    map.insert("SUMMON", Box::new(SummonHandler));
    map.insert("USERS", Box::new(UsersHandler));
}
