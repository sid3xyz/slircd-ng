//! Core handler infrastructure.
//!
//! This module contains the fundamental types and infrastructure for the
//! command handler system, including the handler registry, context types,
//! and middleware for response routing.

pub mod context;
pub mod middleware;
pub mod registry;

// Re-export commonly used types
pub use context::{
    Context, Handler, HandlerError, HandlerResult, HandshakeState, get_nick_or_star, get_oper_info,
    require_oper, require_registered, resolve_nick_to_uid, user_mask_from_state,
};
pub use middleware::ResponseMiddleware;
pub use registry::Registry;
