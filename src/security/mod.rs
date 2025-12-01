//! Security module for slircd-ng.
//!
//! Provides core security features:
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
//! ├─────────────┬────────────────┬──────────────┬────────────────────┤
//! │  Cloaking   │ Rate Limiting  │   X-Lines    │  Spam Detection    │
//! │ HMAC-SHA256 │   Governor     │ K/G/Z/R/S    │  Entropy/Keywords  │
//! │ IP+Hostname │ Token Bucket   │ ExtendedBans │  URL/Repetition    │
//! └─────────────┴────────────────┴──────────────┴────────────────────┘
//! ```

pub mod cloaking;
pub mod extban;
pub mod spam;
pub mod rate_limit;
pub mod xlines;

// Re-export primary types for convenience
#[allow(unused_imports)]
pub use cloaking::{cloak_hostname, cloak_ip_hmac};
pub use rate_limit::RateLimitManager;
pub use xlines::{ExtendedBan, UserContext, XLine, matches_extended_ban, matches_xline};
