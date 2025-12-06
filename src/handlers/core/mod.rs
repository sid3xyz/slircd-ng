//! Core handler infrastructure.
//!
//! This module contains the fundamental types and infrastructure for the
//! command handler system, including the handler registry, context types,
//! and middleware for response routing.
//!
//! ## Typestate Handler Traits (Innovation 1)
//!
//! The `traits` submodule provides state-aware handler traits:
//! - [`PreRegHandler`]: For commands valid before registration
//! - [`PostRegHandler`]: For commands requiring registration
//! - [`UniversalHandler`]: For commands valid in any state
//!
//! See [`traits`] for migration guidance.

pub mod context;
pub mod middleware;
pub mod registry;
pub mod traits;

// Re-export commonly used types
pub use context::{
    Context, Handler, HandlerError, HandlerResult, HandshakeState, get_nick_or_star, get_oper_info,
    is_user_in_channel, require_oper, require_registered, resolve_nick_to_uid,
    user_mask_from_state,
};
pub use middleware::ResponseMiddleware;
pub use registry::Registry;

// Re-export typestate handler traits (Innovation 1)
pub use traits::{
    HandlerPhase, PostRegHandler, PreRegHandler, UniversalHandler, command_phase,
};
