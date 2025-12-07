//! Operator command handlers split into submodules.

mod admin;
mod auth;
mod chghost;
mod kill;
mod trace;
mod vhost;
mod wallops;

pub use admin::{DieHandler, RehashHandler, RestartHandler};
pub use auth::OperHandler;
pub use chghost::ChghostHandler;
pub use kill::KillHandler;
pub use trace::TraceHandler;
pub use vhost::VhostHandler;
pub use wallops::WallopsHandler;

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
