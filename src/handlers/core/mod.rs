//! Core handler infrastructure.
//!
//! This module contains the fundamental types and infrastructure for the
//! command handler system, including the handler registry, context types,
//! and middleware for response routing.
//!
//! ## Typestate Handler System (Innovation 1)
//!
//! ### Phase 1: Registry Enforcement ✅ COMPLETE
//!
//! The Registry implements phase-separated dispatch using three handler maps:
//! - `pre_reg_handlers`: Commands valid before registration (NICK, USER, etc.)
//! - `post_reg_handlers`: Commands requiring registration (PRIVMSG, JOIN, etc.)
//! - `universal_handlers`: Commands valid in any state (QUIT, PING, PONG)
//!
//! Post-registration handlers are structurally inaccessible to unregistered
//! clients - the dispatch path simply doesn't include them.
//!
//! ### Phase 2: Type-Level Enforcement ✅ COMPLETE
//!
//! The `traits` submodule provides **compile-time** protocol state guarantees:
//!
//! - [`TypedContext<S>`]: Context wrapper with state encoded in the type system
//! - [`StatefulPostRegHandler`]: Handlers that receive `TypedContext<Registered>`
//! - [`RegisteredHandlerAdapter`]: Bridge from new traits to legacy `Handler`
//!
//! With `TypedContext<Registered>`, the compiler guarantees:
//! - `ctx.nick()` returns `&str` (not `Option`) - nick is always present
//! - `ctx.user()` returns `&str` (not `Option`) - user is always present
//! - Handler cannot be called with unregistered connection

pub mod context;
pub mod middleware;
pub mod registry;
pub mod traits;

// Re-export commonly used types
pub use context::{
    Context, Handler, HandlerError, HandlerResult, HandshakeState, get_nick_or_star, get_oper_info,
    is_user_in_channel, require_registered, resolve_nick_to_uid,
    user_mask_from_state,
};
pub use middleware::ResponseMiddleware;
pub use registry::Registry;

// Re-export typestate handler traits (Innovation 1 - Phase 1)
pub use traits::{
    HandlerPhase, PostRegHandler, PreRegHandler, UniversalHandler, command_phase,
};

// Re-export compile-time typestate types (Innovation 1 - Phase 2)
pub use traits::{
    RegisteredHandlerAdapter, StatefulPostRegHandler, StatefulPreRegHandler,
    StatefulUniversalHandler, TypedContext, wrap_pre_reg, wrap_registered,
};
