//! Operator command handlers split into submodules.

mod admin;
mod auth;
mod chghost;
mod chgident;
mod globops;
mod kill;
mod spamconf;
mod trace;
mod vhost;
mod wallops;

pub use admin::{DieHandler, RehashHandler, RestartHandler};
pub use auth::OperHandler;
pub use chghost::ChghostHandler;
pub use chgident::ChgIdentHandler;
pub use globops::GlobOpsHandler;
pub use kill::KillHandler;
pub use spamconf::SpamConfHandler;
pub use trace::TraceHandler;
pub use vhost::VhostHandler;
pub use wallops::WallopsHandler;

use crate::handlers::PostRegHandler;
use std::collections::HashMap;

/// Register all operator commands.
pub fn register(map: &mut HashMap<&'static str, Box<dyn PostRegHandler>>) {
    map.insert("OPER", Box::new(OperHandler));
    map.insert("KILL", Box::new(KillHandler));
    map.insert("WALLOPS", Box::new(WallopsHandler));
    map.insert("GLOBOPS", Box::new(GlobOpsHandler));
    map.insert("DIE", Box::new(DieHandler));
    map.insert("REHASH", Box::new(RehashHandler));
    map.insert("RESTART", Box::new(RestartHandler));
    map.insert("CHGHOST", Box::new(ChghostHandler));
    map.insert("CHGIDENT", Box::new(ChgIdentHandler));
    map.insert("VHOST", Box::new(VhostHandler));
    map.insert("TRACE", Box::new(TraceHandler));
    map.insert("SPAMCONF", Box::new(SpamConfHandler));
}

/// Validate hostname per RFC 952/1123 rules.
pub(super) fn is_valid_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.len() > 253 {
        return false;
    }

    if hostname.starts_with('.') || hostname.ends_with('.') {
        return false;
    }

    let labels: Vec<&str> = hostname.split('.').collect();

    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }

        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }

        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
    }

    true
}
