//! Security module for slircd-ng.
//!
//! Provides core security features:
//! - **Ban Cache**: In-memory cache for fast connection-time ban checks
//! - **Cloaking**: HMAC-SHA256 based IP/hostname privacy protection
//! - **Rate Limiting**: Governor-based flood protection for messages, connections, joins
//! - **X-Lines**: Extended ban types (K/G/Z/R/S-lines) for server-level moderation
//! - **Spam Detection**: Multi-layer content analysis for spam prevention
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────┐
//! │                       Security Module                            │
//! ├──────────┬─────────────┬────────────────┬──────────────┬─────────┤
//! │ BanCache │  Cloaking   │ Rate Limiting  │   X-Lines    │  Spam   │
//! │ DashMap  │ HMAC-SHA256 │   Governor     │ K/G/Z/R/S    │ Entropy │
//! │ K/D/G/Z  │ IP+Hostname │ Token Bucket   │ ExtendedBans │ URL/Rep │
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
pub use xlines::{ExtendedBan, UserContext, XLine, matches_extended_ban, matches_xline};
