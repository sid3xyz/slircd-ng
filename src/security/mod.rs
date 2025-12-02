//! Security module for slircd-ng.
//!
//! Provides core security features:
//! - **Ban Cache**: In-memory cache for fast connection-time ban checks (K/G/Z/D-lines)
//! - **Cloaking**: HMAC-SHA256 based IP/hostname privacy protection
//! - **Rate Limiting**: Governor-based flood protection for messages, connections, joins
//! - **Extended Bans**: Pattern matching beyond nick!user@host for channel bans
//! - **Spam Detection**: Multi-layer content analysis for spam prevention
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────┐
//! │                       Security Module                            │
//! ├──────────┬─────────────┬────────────────┬──────────────┬─────────┤
//! │ BanCache │  Cloaking   │ Rate Limiting  │ ExtendedBans │  Spam   │
//! │ DashMap  │ HMAC-SHA256 │   Governor     │ $a:/$r:/$U   │ Entropy │
//! │ K/D/G/Z  │ IP+Hostname │ Token Bucket   │ Channel +b   │ URL/Rep │
//! └──────────┴─────────────┴────────────────┴──────────────┴─────────┘
//! ```

pub mod ban_cache;
pub mod cloaking;
pub mod spam;
pub mod rate_limit;
pub mod xlines;

// Re-export primary types for convenience
pub use ban_cache::BanCache;
#[allow(unused_imports)] // BanResult and BanType used in gateway.rs
pub use ban_cache::{BanResult, BanType};
#[allow(unused_imports)]
pub use cloaking::{cloak_hostname, cloak_ip_hmac};
pub use rate_limit::RateLimitManager;
pub use xlines::{ExtendedBan, UserContext, matches_extended_ban};
