//! Capability-Based Actor Permissions (INNOVATION_4).
//!
//! This module implements unforgeable capability tokens for authorization.
//! Instead of scattered `if is_oper()` checks, functions require possession
//! of a `Cap<T>` token to perform privileged actions.
//!
//! # Architecture
//!
//! The system has three core components:
//!
//! 1. **[`Cap<T>`](tokens::Cap)** - An unforgeable capability token proving authorization.
//!    Can only be created by [`CapabilityAuthority`](authority::CapabilityAuthority).
//!
//! 2. **[`Capability`](tokens::Capability)** - Trait implemented by capability types
//!    (e.g., `KickCap`, `KillCap`, `BanCap`).
//!
//! 3. **[`CapabilityAuthority`](authority::CapabilityAuthority)** - The capability mint.
//!    Evaluates permission based on user/channel state and issues tokens.
//!
//! # Security Properties
//!
//! - `Cap::new()` is `pub(super)` - only the Authority can mint tokens
//! - `Cap<T>` is `!Clone` and `!Copy` - prevents token leakage
//! - Tokens are scoped to specific resources (e.g., channel name)
//! - All capability grants are logged for audit
//!
//! # Usage
//!
//! ```ignore
//! // In a handler:
//! async fn handle_kick(&self, ctx: &Context, channel: &str, target: &str) -> Result<()> {
//!     // Request capability from authority
//!     let authority = CapabilityAuthority::new(ctx.matrix.clone());
//!     let kick_cap = authority.request_kick_cap(ctx.uid, channel).await
//!         .ok_or_else(|| HandlerError::NoPrivileges)?;
//!
//!     // Perform the kick - function signature proves authorization
//!     channel_actor.kick(target, kick_cap).await?;
//!     Ok(())
//! }
//! ```
//!
//! # Migration Status
//!
//! - Phase 1 ✅ Core types and Authority
//! - Phase 2 ✅ 9 handler files migrated to use capability tokens:
//!   - `handlers/admin.rs` - SA* commands
//!   - `handlers/oper/kill.rs` - KILL command
//!   - `handlers/oper/vhost.rs` - VHOST command
//!   - `handlers/oper/trace.rs` - TRACE command
//!   - `handlers/oper/chghost.rs` - CHGHOST command
//!   - `handlers/oper/wallops.rs` - WALLOPS command
//!   - `handlers/oper/admin.rs` - DIE/REHASH/RESTART commands
//!   - `handlers/bans/shun.rs` - SHUN/UNSHUN commands
//!   - `handlers/bans/xlines/mod.rs` - K/G/D/Z/R-LINE commands
//! - Phase 3 ✅ Channel handlers migrated (3 handlers):
//!   - `handlers/channel/kick.rs` - KICK command
//!   - `handlers/channel/topic.rs` - TOPIC command  
//!   - `handlers/channel/invite.rs` - INVITE command

// Allow dead_code for capability types not yet used by handlers (Phase 3 pending)
#![allow(dead_code)]

mod authority;
mod irc;
mod tokens;

// Re-export authority (used by 9 handler files)
pub use authority::CapabilityAuthority;

// Re-export capability types - channel caps migrated in Phase 3
#[allow(unused_imports)]
pub use irc::{
    // Channel capabilities (Phase 3 migrated)
    BanCap, InviteCap, KickCap, TopicCap, ChannelModeCap, VoiceCap,
    // Oper capabilities (in use)
    KillCap, KlineCap, DlineCap, GlineCap, RehashCap, DieCap, RestartCap,
    // Special capabilities
    BypassFloodCap, BypassModeCap, GlobalNoticeCap,
};
#[allow(unused_imports)]
pub use tokens::{Cap, Capability};
