//! Operator command handlers split into submodules.

mod auth;
mod chghost;
mod chgident;
mod clearchan;
mod connect;
mod globops;
mod kill;
mod lifecycle;
mod spamconf;
mod trace;
mod vhost;
mod wallops;

pub use auth::OperHandler;
pub use chghost::ChghostHandler;
pub use chgident::ChgIdentHandler;
pub use clearchan::ClearchanHandler;
pub use connect::ConnectHandler;
pub use globops::GlobOpsHandler;
pub use kill::KillHandler;
pub use lifecycle::{DieHandler, RehashHandler, RestartHandler};
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
    map.insert("CLEARCHAN", Box::new(ClearchanHandler));
    map.insert("CONNECT", Box::new(ConnectHandler));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_hostname_simple() {
        assert!(is_valid_hostname("example"));
        assert!(is_valid_hostname("localhost"));
        assert!(is_valid_hostname("server1"));
    }

    #[test]
    fn test_is_valid_hostname_multi_label() {
        assert!(is_valid_hostname("irc.example.com"));
        assert!(is_valid_hostname("mail.server.example.org"));
        assert!(is_valid_hostname("a.b.c.d.e.f"));
    }

    #[test]
    fn test_is_valid_hostname_empty() {
        assert!(!is_valid_hostname(""));
    }

    #[test]
    fn test_is_valid_hostname_too_long() {
        // Create a hostname that is exactly 254 characters (too long)
        let long_label = "a".repeat(63);
        let long_hostname = format!(
            "{}.{}.{}.{}",
            long_label, long_label, long_label, long_label
        );
        assert!(long_hostname.len() > 253);
        assert!(!is_valid_hostname(&long_hostname));

        // 253 chars should be the limit
        let exactly_253 = "a".repeat(253);
        // This would fail label length check anyway, but hostname length is checked first
        assert!(!is_valid_hostname(&exactly_253)); // 253-char single label > 63
    }

    #[test]
    fn test_is_valid_hostname_starts_with_dot() {
        assert!(!is_valid_hostname(".example.com"));
        assert!(!is_valid_hostname(".localhost"));
    }

    #[test]
    fn test_is_valid_hostname_ends_with_dot() {
        assert!(!is_valid_hostname("example.com."));
        assert!(!is_valid_hostname("localhost."));
    }

    #[test]
    fn test_is_valid_hostname_label_too_long() {
        // 64 characters in a label is too long
        let long_label = "a".repeat(64);
        assert!(!is_valid_hostname(&long_label));
        assert!(!is_valid_hostname(&format!("{}.example.com", long_label)));

        // 63 characters is the maximum allowed
        let max_label = "a".repeat(63);
        assert!(is_valid_hostname(&max_label));
        assert!(is_valid_hostname(&format!("{}.example.com", max_label)));
    }

    #[test]
    fn test_is_valid_hostname_label_starts_with_hyphen() {
        assert!(!is_valid_hostname("-example"));
        assert!(!is_valid_hostname("-example.com"));
        assert!(!is_valid_hostname("www.-example.com"));
    }

    #[test]
    fn test_is_valid_hostname_label_ends_with_hyphen() {
        assert!(!is_valid_hostname("example-"));
        assert!(!is_valid_hostname("example-.com"));
        assert!(!is_valid_hostname("www.example-.com"));
    }

    #[test]
    fn test_is_valid_hostname_invalid_characters() {
        assert!(!is_valid_hostname("example_host")); // underscore
        assert!(!is_valid_hostname("example host")); // space
        assert!(!is_valid_hostname("example@host")); // at sign
        assert!(!is_valid_hostname("example!host")); // exclamation
        assert!(!is_valid_hostname("example#host")); // hash
        assert!(!is_valid_hostname("example.com/path")); // slash
        assert!(!is_valid_hostname("日本語.com")); // non-ASCII
    }

    #[test]
    fn test_is_valid_hostname_hyphen_in_middle() {
        assert!(is_valid_hostname("my-server"));
        assert!(is_valid_hostname("irc-server.example.com"));
        assert!(is_valid_hostname("a-b-c-d"));
        assert!(is_valid_hostname("my-cool-irc-server.network.org"));
    }

    #[test]
    fn test_is_valid_hostname_empty_label() {
        assert!(!is_valid_hostname("example..com")); // empty label between dots
        assert!(!is_valid_hostname("..example")); // empty labels at start
    }

    #[test]
    fn test_is_valid_hostname_numeric() {
        assert!(is_valid_hostname("123"));
        assert!(is_valid_hostname("192-168-1-1"));
        assert!(is_valid_hostname("server1.example.com"));
    }
}
