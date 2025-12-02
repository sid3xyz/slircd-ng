//! Security module for slircd-ng.
//!
//! Provides core security features:
//! - **IP Deny List**: High-performance Roaring Bitmap engine for nanosecond IP rejection
//! - **Ban Cache**: In-memory cache for fast connection-time ban checks (K/G/Z/D-lines)
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
//! │ MsgPack    │ K/D/G/Z  │ IP+Hostname │ Token Bucket   │ Chan +b  │ URL/Rep │
//! └────────────┴──────────┴─────────────┴────────────────┴──────────┴─────────┘
//! ```

pub mod ban_cache;
pub mod cloaking;
pub mod ip_deny_list;
pub mod rate_limit;
pub mod spam;
pub mod xlines;

// Re-export primary types for convenience
pub use ban_cache::BanCache;
#[allow(unused_imports)] // BanResult and BanType used in gateway.rs
pub use ban_cache::{BanResult, BanType};
#[allow(unused_imports)]
pub use cloaking::{cloak_hostname, cloak_ip_hmac};
#[allow(unused_imports)] // IpDenyList used in Phase 2-4 (Matrix, Gateway, Handlers)
pub use ip_deny_list::{BanMetadata, IpDenyList};
pub use rate_limit::RateLimitManager;
pub use xlines::{ExtendedBan, UserContext, matches_extended_ban};
