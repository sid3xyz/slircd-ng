//! Server query handlers: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD, MAP, RULES, USERIP, LINKS, HELP
//!
//! RFC 2812 §3.4 - Server queries and commands
//! RFC 2812 §3.5 - Service queries (SERVLIST, SQUERY)

pub use disabled::{SummonHandler, UsersHandler};
pub use help::HelpHandler;
pub use info::InfoHandler;
pub use lusers::LusersHandler;
pub use motd::MotdHandler;
pub use rules::RulesHandler;
pub use service::ServiceHandler;
pub use stats::StatsHandler;
pub use time::TimeHandler;
pub use userip::UseripHandler;
pub use version::VersionHandler;

mod admin;
mod disabled;
mod help;
mod info;
mod lusers;
mod motd;
mod rules;
mod service;
mod stats;
mod time;
mod userip;
mod version;

use crate::handlers::{PostRegHandler, s2s};
use std::collections::HashMap;

/// Register server query handlers.
pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("VERSION", Box::new(VersionHandler));
    map.insert("TIME", Box::new(TimeHandler));
    map.insert("ADMIN", Box::new(admin::AdminHandler));
    map.insert("INFO", Box::new(InfoHandler));
    map.insert("LUSERS", Box::new(LusersHandler));
    map.insert("STATS", Box::new(StatsHandler));
    map.insert("MOTD", Box::new(MotdHandler));
    map.insert("MAP", Box::new(s2s::MapHandler));
    map.insert("RULES", Box::new(RulesHandler));
    map.insert("USERIP", Box::new(UseripHandler));
    map.insert("LINKS", Box::new(s2s::LinksHandler));
    map.insert("HELP", Box::new(HelpHandler));
    map.insert("SERVICE", Box::new(ServiceHandler));
    map.insert("SQUERY", Box::new(service::SqueryHandler));
    map.insert("SERVLIST", Box::new(service::ServlistHandler));
    map.insert("SUMMON", Box::new(SummonHandler));
    map.insert("USERS", Box::new(UsersHandler));
    map.insert("CONNECT", Box::new(s2s::ConnectHandler));
    map.insert("SQUIT", Box::new(s2s::SquitHandler));
}
