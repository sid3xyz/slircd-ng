//! State-aware handler traits for typestate protocol enforcement (Innovation 1).
//!
//! This module defines handler traits that encode registration requirements
//! at the type level, making it impossible to invoke post-registration handlers
//! on unregistered connections.
//!
//! ## Handler Types
//!
//! - [`PreRegHandler`]: Commands valid before registration (NICK, USER, CAP, etc.)
//! - [`PostRegHandler`]: Commands requiring registration (PRIVMSG, JOIN, etc.)
//! - [`UniversalHandler`]: Commands valid in any state (QUIT, PING, PONG)
//!
//! ## Context<S> Type Parameter
//!
//! Post-registration handlers receive `Context<'_, RegisteredState>`, which
//! provides compile-time guarantees that nick/user are present. Pre-registration
//! handlers receive `Context<'_, UnregisteredState>` where nick/user are optional.
//!
//! ## Registration Guarantees
//!
//! When you have a `Context<'_, RegisteredState>`, the type system guarantees:
//! - `ctx.state.nick` is `String` (not `Option<String>`)
//! - `ctx.state.user` is `String` (not `Option<String>`)
//! - The client has completed the full registration handshake

use super::context::{Context, HandlerResult};
use crate::state::{RegisteredState, ServerState, SessionState, UnregisteredState};
use async_trait::async_trait;
use slirc_proto::MessageRef;

// ============================================================================
// Handler Traits (Innovation 1)
// ============================================================================

/// Handler for commands valid BEFORE registration (NICK, USER, CAP, PASS).
/// Receives `Context<'_, UnregisteredState>`.
#[async_trait]
pub trait PreRegHandler: Send + Sync {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Handler for commands requiring FULL registration (PRIVMSG, JOIN, etc.).
/// Receives `Context<'_, RegisteredState>` — nick/user guaranteed present.
#[async_trait]
pub trait PostRegHandler: Send + Sync {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Handler for commands from other SERVERS (BURST, DELTA, etc.).
/// Receives `Context<'_, ServerState>`.
#[async_trait]
pub trait ServerHandler: Send + Sync {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Handler for commands valid in ANY state (QUIT, PING, PONG, NICK, CAP).
///
/// These handlers are generic over the session state type `S`, which must
/// implement `SessionState`. This allows them to work with both pre-registration
/// and post-registration connections using the common interface.
///
/// The `SessionState` trait provides:
/// - `nick()` → `Option<&str>` (always `Some` for RegisteredState)
/// - `capabilities()` → `&HashSet<String>`
/// - `is_registered()` → `bool`
/// - etc.
#[async_trait]
pub trait UniversalHandler<S: SessionState>: Send + Sync {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult;
}

/// Object-safe trait for universal handlers that can be stored in the registry.
///
/// This trait provides dispatch methods for both state types, allowing a single
/// handler object to work with both pre-registration and post-registration contexts.
#[async_trait]
pub trait DynUniversalHandler: Send + Sync {
    /// Handle a command in pre-registration state.
    async fn handle_unreg(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;

    /// Handle a command in post-registration state.
    async fn handle_reg(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;

    /// Handle a command in server state.
    async fn handle_server(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Blanket implementation: any type that implements UniversalHandler for both
/// state types automatically implements DynUniversalHandler.
#[async_trait]
impl<T> DynUniversalHandler for T
where
    T: UniversalHandler<UnregisteredState>
        + UniversalHandler<RegisteredState>
        + UniversalHandler<ServerState>
        + Send
        + Sync,
{
    async fn handle_unreg(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<UnregisteredState>>::handle(self, ctx, msg).await
    }

    async fn handle_reg(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<RegisteredState>>::handle(self, ctx, msg).await
    }

    async fn handle_server(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<ServerState>>::handle(self, ctx, msg).await
    }
}

// ============================================================================
// Blanket Implementations for PreRegHandler
// ============================================================================

/// A handler that implements UniversalHandler<UnregisteredState> can act as PreRegHandler.
#[async_trait]
impl<T: UniversalHandler<UnregisteredState>> PreRegHandler for T {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<UnregisteredState>>::handle(self, ctx, msg).await
    }
}

/// A handler that implements UniversalHandler<RegisteredState> can act as PostRegHandler.
#[async_trait]
impl<T: UniversalHandler<RegisteredState>> PostRegHandler for T {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<RegisteredState>>::handle(self, ctx, msg).await
    }
}

/// A handler that implements UniversalHandler<ServerState> can act as ServerHandler.
#[async_trait]
impl<T: UniversalHandler<ServerState>> ServerHandler for T {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        <T as UniversalHandler<ServerState>>::handle(self, ctx, msg).await
    }
}
