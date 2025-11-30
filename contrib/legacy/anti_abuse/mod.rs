//! Anti-abuse module - preventing IRC abuse through multiple mechanisms

mod primitives;
mod service;
mod spam_detection;

// Re-export primitives for backward compatibility
pub use primitives::*;

// Export service API
pub use service::{AbuseReason, AbuseResult, AntiAbuseService, ZLineRecord};

// Export spam detection service
pub use spam_detection::{SpamDetectionService, SpamVerdict};
