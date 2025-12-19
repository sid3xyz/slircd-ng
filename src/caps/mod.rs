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
//! All handlers using capability-based authorization have been migrated.
//!
//! NOTE: Some capability types (BanCap, VoiceCap, ChannelModeCap, BypassFloodCap,
//! BypassModeCap, GlobalNoticeCap) are defined but not yet used. These form a complete
//! API for future integration when channel/oper operations are refactored to use
//! capability tokens uniformly.

#![allow(dead_code)]

mod authority;
mod irc;
mod tokens;

// Re-export authority (used by 9 handler files)
pub use authority::CapabilityAuthority;
pub use tokens::Cap;
pub use irc::*;
