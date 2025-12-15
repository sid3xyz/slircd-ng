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
pub mod ip_deny;
pub mod rate_limit;
pub mod spam;
pub mod xlines;
pub mod reputation;
pub mod dnsbl;
pub mod heuristics;

// Re-export primary types for convenience
pub use ban_cache::BanCache;
#[allow(unused_imports)] // BanResult/BanType used by welcome.rs
pub use ban_cache::{BanResult, BanType};
#[allow(unused_imports)]
pub use cloaking::{cloak_hostname, cloak_ip_hmac};
#[allow(unused_imports)] // BanMetadata used by STATS command
pub use ip_deny::{BanMetadata, IpDenyList};
pub use rate_limit::RateLimitManager;
pub use xlines::{ExtendedBan, UserContext, matches_extended_ban};
pub use reputation::ReputationManager;
pub use dnsbl::DnsblService;
pub use heuristics::HeuristicsEngine;

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
