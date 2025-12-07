//! Example handlers demonstrating the Phase 2 typestate system.
//!
//! This module contains example implementations showing how to migrate
//! handlers to the new compile-time safe `StatefulPostRegHandler` trait.

#![allow(dead_code)] // Example code for documentation

use super::context::HandlerResult;
use super::traits::{PostRegHandler, TypedContext};
use crate::state::Registered;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Example: VERSION handler using StatefulPostRegHandler.
///
/// This demonstrates the compile-time safety provided by `TypedContext<Registered>`:
/// - `ctx.nick()` returns `&str`, not `Option<&str>`
/// - The type system guarantees we're registered before this code runs
///
/// # Comparison
///
/// ## Before (legacy Handler trait)
/// ```ignore
/// impl Handler for VersionHandler {
///     async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
///         let nick = ctx.state.nick.as_ref()
///             .ok_or(HandlerError::NickOrUserMissing)?;  // Runtime check!
///         // ...
///     }
/// }
/// ```
///
/// ## After (StatefulPostRegHandler trait)
/// ```ignore
/// impl StatefulPostRegHandler for VersionHandlerStateful {
///     async fn handle_registered(
///         &self,
///         ctx: &mut TypedContext<'_, Registered>,
///         _msg: &MessageRef<'_>,
///     ) -> HandlerResult {
///         let nick = ctx.nick();  // Compile-time guarantee: always valid!
///         // ...
///     }
/// }
/// ```
pub struct VersionHandlerStateful;

/// Server version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[async_trait]
impl PostRegHandler for VersionHandlerStateful {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!

        let server_name = &ctx.inner().matrix.server_info.name;

        // RPL_VERSION (351): <version>.<debuglevel> <server> :<comments>
        #[cfg(debug_assertions)]
        let version_str = format!("{}-debug.1", VERSION);
        #[cfg(not(debug_assertions))]
        let version_str = format!("{}.0", VERSION);

        ctx.inner()
            .send_reply(
                Response::RPL_VERSION,
                vec![
                    nick.to_string(),
                    version_str,
                    server_name.clone(),
                    "slircd-ng IRC daemon (typestate example)".to_string(),
                ],
            )
            .await?;

        Ok(())
    }
}

// ============================================================================
// Registration with legacy Registry
// ============================================================================
//
// To use StatefulPostRegHandler handlers with the existing Registry:
//
// ```ignore
// use crate::handlers::core::{RegisteredHandlerAdapter, StatefulPostRegHandler};
//
// // In Registry::new():
// post_reg_handlers.insert(
//     "VERSION",
//     Box::new(RegisteredHandlerAdapter(VersionHandlerStateful))
// );
// ```
//
// The adapter wraps the compile-time-safe handler and:
// 1. Performs a runtime registration check (belt-and-suspenders safety)
// 2. Creates a TypedContext<Registered>
// 3. Calls the type-safe handler
//
// This allows gradual migration from Handler -> StatefulPostRegHandler.

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time test: demonstrate that TypedContext<Registered>
    /// provides non-Option accessors for nick/user.
    ///
    /// This test doesn't need to run - if it compiles, it proves
    /// the type system is working correctly.
    #[allow(dead_code)]
    fn demonstrate_compile_time_safety() {
        // This function signature proves the types work:
        // - TypedContext<Registered> provides .nick() -> &str
        // - Not Option<&str>, so no unwrapping needed

        fn _uses_nick(_nick: &str) {}

        // If TypedContext is properly constrained, this should compile:
        fn _handler_uses_nick(ctx: &TypedContext<'_, Registered>) {
            let nick: &str = ctx.nick(); // No .unwrap() needed!
            _uses_nick(nick);
        }
    }
}
