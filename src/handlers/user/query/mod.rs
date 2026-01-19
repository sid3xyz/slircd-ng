use crate::handlers::PostRegHandler;
use std::collections::HashMap;

pub mod who;
pub mod whois;

/// Register user query handlers.
pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("WHO", Box::new(who::WhoHandler));
    map.insert("WHOIS", Box::new(whois::WhoisHandler));
    map.insert("WHOWAS", Box::new(whois::WhowasHandler));
    map.insert("ISON", Box::new(whois::IsonHandler));
    map.insert("USERHOST", Box::new(whois::UserhostHandler));
}
