//! User query handlers: WHO, WHOIS, WHOWAS, USERHOST, ISON
//!
//! RFC 2812 §3.6 - User based queries

mod who;
mod whois;

pub use who::WhoHandler;
pub use whois::{IsonHandler, UserhostHandler, WhoisHandler, WhowasHandler};

use std::collections::HashMap;
use crate::handlers::PostRegHandler;

pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("WHO", Box::new(WhoHandler));
    map.insert("ISON", Box::new(IsonHandler));
    map.insert("USERHOST", Box::new(UserhostHandler));
    map.insert("WHOIS", Box::new(WhoisHandler));
    map.insert("WHOWAS", Box::new(WhowasHandler));
}
