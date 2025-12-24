//! Security module for slircd-ng.
//!
//! Provides core security features:
//! - **IP Deny List**: High-performance Roaring Bitmap engine for nanosecond IP rejection
//! - **Ban Cache**: In-memory cache for fast connection-time ban checks (K/G-lines)
//! - **Cloaking**: HMAC-SHA256 based IP/hostname privacy protection
//! - **Rate Limiting**: Governor-based flood protection for messages, connections, joins
//! - **Extended Bans**: Pattern matching beyond nick!user@host for channel bans
//! - **Spam Detection**: Multi-layer content analysis for spam prevention
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────────────┐
//! │                           Security Module                                 │
//! ├────────────┬──────────┬─────────────┬────────────────┬──────────┬─────────┤
//! │ IpDenyList │ BanCache │  Cloaking   │ Rate Limiting  │ ExtBans  │  Spam   │
//! │ RoaringBmp │ DashMap  │ HMAC-SHA256 │   Governor     │ $a:/$r:  │ Entropy │
//! │ Z/D-lines  │  K/G     │ IP+Hostname │ Token Bucket   │ Chan +b  │ URL/Rep │
//! └────────────┴──────────┴─────────────┴────────────────┴──────────┴─────────┘
//! ```

pub mod ban_cache;
pub mod cloaking;
pub mod dnsbl;
pub mod heuristics;
pub mod ip_deny;
pub mod rate_limit;
pub mod reputation;
pub mod spam;
pub mod xlines;

// Re-export primary types for convenience
pub use ban_cache::BanCache;
pub use dnsbl::DnsblService;
pub use heuristics::HeuristicsEngine;
pub use rate_limit::RateLimitManager;
pub use reputation::ReputationManager;
pub use xlines::{ExtendedBan, RegistrationParams, UserContext, matches_extended_ban};

// Re-export hostmask matching from proto for consistent IRC pattern matching
pub use slirc_proto::matches_hostmask;

/// Check if a ban/exception entry matches a user, supporting both hostmask and extended bans.
///
/// This is the unified helper used by JOIN and speak paths for consistent extended ban handling.
///
/// # Arguments
/// * `mask` - The ban mask (either nick!user@host or $type:pattern)
/// * `user_mask` - The user's full hostmask (nick!user@host)
/// * `user_context` - Full user context for extended ban matching
pub fn matches_ban_or_except(mask: &str, user_mask: &str, user_context: &UserContext) -> bool {
    if mask.starts_with('$') {
        // Extended ban format ($a:account, $r:realname, etc.)
        if let Some(extban) = ExtendedBan::parse(mask) {
            matches_extended_ban(&extban, user_context)
        } else {
            false
        }
    } else {
        // Traditional nick!user@host pattern
        matches_hostmask(mask, user_mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test UserContext with sensible defaults
    fn test_user_context() -> UserContext {
        UserContext {
            nickname: "TestNick".to_string(),
            username: "testuser".to_string(),
            hostname: "host.example.com".to_string(),
            realname: "Test Real Name".to_string(),
            account: Some("myaccount".to_string()),
            server: "irc.example.net".to_string(),
            channels: vec!["#test".to_string(), "#help".to_string()],
            is_oper: false,
            oper_type: None,
            certificate_fp: None,
            sasl_mechanism: None,
            is_registered: true,
            is_tls: false,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Traditional hostmask tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn matches_ban_traditional_hostmask_match() {
        let ctx = test_user_context();
        let mask = "*!*@*.example.com";
        let user_mask = "TestNick!testuser@host.example.com";
        assert!(matches_ban_or_except(mask, user_mask, &ctx));
    }

    #[test]
    fn matches_ban_traditional_hostmask_no_match() {
        let ctx = test_user_context();
        let mask = "*!*@*.other.com";
        let user_mask = "TestNick!testuser@host.example.com";
        assert!(!matches_ban_or_except(mask, user_mask, &ctx));
    }

    #[test]
    fn matches_ban_wildcard_all() {
        let ctx = test_user_context();
        let mask = "*!*@*";
        let user_mask = "AnyNick!anyuser@any.host.here";
        assert!(matches_ban_or_except(mask, user_mask, &ctx));
    }

    #[test]
    fn matches_ban_exact_hostmask() {
        let ctx = test_user_context();
        let mask = "ExactNick!exactuser@exact.host";
        let user_mask = "ExactNick!exactuser@exact.host";
        assert!(matches_ban_or_except(mask, user_mask, &ctx));
    }

    #[test]
    fn matches_ban_nick_wildcard() {
        let ctx = test_user_context();
        let mask = "Test*!*@*";
        let user_mask = "TestNick!testuser@host.example.com";
        assert!(matches_ban_or_except(mask, user_mask, &ctx));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Extended ban tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn matches_ban_extended_account_match() {
        let ctx = test_user_context();
        let mask = "$a:myaccount";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_account_no_match() {
        let ctx = test_user_context();
        let mask = "$a:otheraccount";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_account_wildcard() {
        let ctx = test_user_context();
        let mask = "$a:my*";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_account_no_account() {
        let mut ctx = test_user_context();
        ctx.account = None;
        ctx.is_registered = false;
        let mask = "$a:*";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_realname_match() {
        let ctx = test_user_context();
        let mask = "$r:*Real*";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_realname_no_match() {
        let ctx = test_user_context();
        let mask = "$r:*Bot*";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_unregistered() {
        let mut ctx = test_user_context();
        ctx.account = None;
        ctx.is_registered = false;
        let mask = "$U";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_unregistered_registered_user() {
        let ctx = test_user_context(); // has account
        let mask = "$U";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_invalid_format() {
        let ctx = test_user_context();
        // Invalid extended ban format - missing pattern after colon
        let mask = "$x";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_unknown_type() {
        let ctx = test_user_context();
        // Unknown extended ban type
        let mask = "$q:something";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_server_match() {
        let ctx = test_user_context();
        let mask = "$s:*.example.net";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_channel_match() {
        let ctx = test_user_context();
        let mask = "$c:#test";
        assert!(matches_ban_or_except(mask, "", &ctx));
    }

    #[test]
    fn matches_ban_extended_channel_no_match() {
        let ctx = test_user_context();
        let mask = "$c:#other";
        assert!(!matches_ban_or_except(mask, "", &ctx));
    }
}
