//! Core handler infrastructure.
//!
//! This module contains the fundamental types and infrastructure for the
//! command handler system, including the handler registry, context types,
//! and middleware for response routing.
//!
//! ## Typestate Handler System (Innovation 1 Phase 3)
//!
//! The handler system enforces protocol state at compile time using the type system.
//!
//! ### Handler Traits
//!
//! - [`PreRegHandler`]: Commands valid before registration (NICK, USER, etc.)
//!   - Receives `Context<'_, UnregisteredState>`
//! - [`PostRegHandler`]: Commands requiring registration (PRIVMSG, JOIN, etc.)
//!   - Receives `Context<'_, RegisteredState>` with guaranteed nick/user
//! - [`UniversalHandler<S>`]: Commands valid in any state (QUIT, PING, PONG)
//!   - Generic over `S: SessionState`, works with both states
//! - [`DynUniversalHandler`]: Object-safe version for dynamic dispatch
//!
//! ### State Types
//!
//! - `UnregisteredState`: Pre-registration, nick/user are `Option<String>`
//! - `RegisteredState`: Post-registration, nick/user are `String` (guaranteed)
//! - `SessionState` trait: Common interface for universal handlers

pub mod context;
pub mod examples;
pub mod middleware;
pub mod registry;
pub mod traits;

// Re-export commonly used types
pub use context::{
    Context, HandlerError, HandlerResult, get_nick_or_star, get_oper_info,
    is_user_in_channel, resolve_nick_to_uid, user_mask_from_state,
};
pub use middleware::ResponseMiddleware;
pub use registry::Registry;

// Re-export typestate handler traits (Innovation 1 Phase 3)
pub use traits::{PostRegHandler, PreRegHandler, UniversalHandler};

// DynUniversalHandler is used internally by Registry
#[allow(unused_imports)]
pub(crate) use traits::DynUniversalHandler;
