//! Example handlers demonstrating the Phase 3 typestate system.
//!
//! This module contains example implementations showing how to use
//! the compile-time safe `PostRegHandler` trait with `Context<RegisteredState>`.

#![allow(dead_code)] // Example code for documentation

use super::context::{Context, HandlerResult};
use super::traits::PostRegHandler;
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Example: VERSION handler using PostRegHandler with RegisteredState.
///
/// This demonstrates the compile-time safety provided by `Context<RegisteredState>`:
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
/// ## After (PostRegHandler with RegisteredState)
/// ```ignore
/// impl PostRegHandler for VersionHandlerStateful {
///     async fn handle(
///         &self,
///         ctx: &mut Context<'_, RegisteredState>,
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
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!

        let server_name = &ctx.matrix.server_info.name;

        // RPL_VERSION (351): <version>.<debuglevel> <server> :<comments>
        #[cfg(debug_assertions)]
        let version_str = format!("{}-debug.1", VERSION);
        #[cfg(not(debug_assertions))]
        let version_str = format!("{}.0", VERSION);

        ctx.send_reply(
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
// Phase 3: Direct Context<RegisteredState>
// ============================================================================
//
// With Phase 3 typestate enforcement, handlers receive `Context<RegisteredState>`
// directly. No wrapper is needed because:
//
// 1. The connection loop uses the `ConnectionState` enum
// 2. Post-registration handlers only receive `Context<RegisteredState>`
// 3. The type itself guarantees nick/user are present (String, not Option)
//
// The compile-time safety comes from the type system, not a runtime wrapper.

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time test: demonstrate that Context<RegisteredState>
    /// provides non-Option accessors for nick/user.
    ///
    /// This test doesn't need to run - if it compiles, it proves
    /// the type system is working correctly.
    #[allow(dead_code)]
    fn demonstrate_compile_time_safety() {
        // This function signature proves the types work:
        // - Context<RegisteredState> provides .nick() -> &str
        // - Not Option<&str>, so no unwrapping needed

        fn _uses_nick(_nick: &str) {}

        // With RegisteredState, this compiles without unwrap:
        fn _handler_uses_nick(ctx: &Context<'_, RegisteredState>) {
            let nick: &str = ctx.nick(); // No .unwrap() needed!
            _uses_nick(nick);
        }
    }
}
