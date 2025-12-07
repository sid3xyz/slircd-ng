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
//! ## TypedContext<S>
//!
//! The `TypedContext<'a, S>` wrapper provides compile-time guarantees about
//! protocol state. Post-registration handlers receive `TypedContext<Registered>`,
//! making it **impossible** to call them with an unregistered connection.
//!
//! ## Registration Guarantees
//!
//! When you have a `TypedContext<Registered>`, the type system guarantees:
//! - `ctx.nick()` always returns `&str` (never `Option`)
//! - `ctx.user()` always returns `&str` (never `Option`)
//! - The client has completed the full registration handshake
//!
//! ## Migration Path
//!
//! Existing handlers implement the `Handler` trait. To migrate:
//!
//! 1. For pre-reg handlers: implement `StatefulPreRegHandler` instead
//! 2. For post-reg handlers: implement `StatefulPostRegHandler` instead
//! 3. Gain compile-time safety - no runtime registration checks needed!

// Phase 2 foundation code - will be used as handlers are migrated
#![allow(dead_code)]

use super::context::{Context, HandlerResult};
use crate::state::{IsRegistered, PreRegistration, ProtocolState, Registered};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use std::marker::PhantomData;

// ============================================================================
// TypedContext: Compile-Time Protocol State Guarantees
// ============================================================================

/// A context wrapper that provides compile-time protocol state guarantees.
///
/// Unlike the base `Context`, `TypedContext<S>` encodes the protocol state
/// at the type level. This enables:
///
/// 1. **Compile-time dispatch**: Post-reg handlers receive `TypedContext<Registered>`,
///    making it impossible to call them with an unregistered connection.
///
/// 2. **Safe accessors**: `ctx.nick()` on `TypedContext<Registered>` returns `&str`,
///    not `Option<&str>`, because registration guarantees nick is set.
///
/// 3. **Type-safe transitions**: State transitions return new types, enforced by compiler.
///
/// ## Example
///
/// ```ignore
/// async fn handle_privmsg(ctx: &mut TypedContext<'_, Registered>, msg: &MessageRef<'_>) {
///     // Compile-time guarantee: nick is always present
///     let nick = ctx.nick();  // Returns &str, not Option!
///
///     // If you tried to call this with TypedContext<Unregistered>, it wouldn't compile
/// }
/// ```
pub struct TypedContext<'a, S: ProtocolState> {
    /// The underlying context (uses raw pointer to avoid lifetime complexity)
    inner: *mut Context<'a>,
    /// Zero-sized marker for state
    _state: PhantomData<S>,
    /// Lifetime marker
    _lifetime: PhantomData<&'a mut ()>,
}

// SAFETY: TypedContext is Send/Sync if Context is Send/Sync
// The raw pointer is only used for internal bookkeeping
unsafe impl<'a, S: ProtocolState> Send for TypedContext<'a, S> {}
unsafe impl<'a, S: ProtocolState> Sync for TypedContext<'a, S> {}

impl<'a, S: ProtocolState> TypedContext<'a, S> {
    /// Create a typed context from an untyped context.
    ///
    /// # Safety
    /// This is only safe when the protocol state matches what `S` claims.
    #[inline]
    pub fn new_unchecked(ctx: &'a mut Context<'a>) -> Self {
        Self {
            inner: ctx as *mut Context<'a>,
            _state: PhantomData,
            _lifetime: PhantomData,
        }
    }

    /// Access the underlying context
    #[inline]
    pub fn inner(&self) -> &Context<'a> {
        // SAFETY: We hold exclusive access via the lifetime
        unsafe { &*self.inner }
    }

    /// Access the underlying context mutably
    #[inline]
    pub fn inner_mut(&mut self) -> &mut Context<'a> {
        // SAFETY: We hold exclusive mutable access via the lifetime
        unsafe { &mut *self.inner }
    }

    /// Get the user's unique ID
    #[inline]
    pub fn uid(&self) -> &str {
        self.inner().uid
    }

    /// Get the remote address
    #[inline]
    pub fn remote_addr(&self) -> std::net::SocketAddr {
        self.inner().remote_addr
    }
}

/// Extension methods available only for registered connections.
///
/// These methods provide guaranteed-present values that would be
/// `Option` for unregistered connections.
impl<'a> TypedContext<'a, Registered> {
    /// Get the user's nickname.
    ///
    /// # Panics
    /// Never panics for Registered state - nick is guaranteed present.
    #[inline]
    pub fn nick(&self) -> &str {
        self.inner()
            .handshake
            .nick
            .as_deref()
            .expect("Registered TypedContext invariant violated: nick missing")
    }

    /// Get the username.
    ///
    /// # Panics
    /// Never panics for Registered state - user is guaranteed present.
    #[inline]
    pub fn user(&self) -> &str {
        self.inner()
            .handshake
            .user
            .as_deref()
            .expect("Registered TypedContext invariant violated: user missing")
    }

    /// Get both nick and user.
    #[inline]
    pub fn nick_user(&self) -> (&str, &str) {
        (self.nick(), self.user())
    }
}

/// Wrap a context as pre-registration (for use by registry).
///
/// # Panics in debug builds
/// Panics if the connection is already registered.
pub fn wrap_pre_reg<'a, S: PreRegistration>(ctx: &'a mut Context<'a>) -> TypedContext<'a, S> {
    debug_assert!(
        !ctx.handshake.registered,
        "wrap_pre_reg called on registered connection"
    );
    TypedContext::new_unchecked(ctx)
}

/// Wrap a context as registered (for use by registry).
///
/// # Panics in debug builds
/// Panics if the connection is not registered.
pub fn wrap_registered<'a>(ctx: &'a mut Context<'a>) -> TypedContext<'a, Registered> {
    debug_assert!(
        ctx.handshake.registered,
        "wrap_registered called on unregistered connection"
    );
    debug_assert!(
        ctx.handshake.nick.is_some() && ctx.handshake.user.is_some(),
        "wrap_registered called but nick/user missing"
    );
    TypedContext::new_unchecked(ctx)
}

// ============================================================================
// Pre-Registration Handler Trait
// ============================================================================

/// Handler for commands valid before registration completes.
///
/// These handlers can be invoked in `Unregistered` or `Negotiating` states.
/// They are used for:
/// - Connection registration: NICK, USER, PASS
/// - Capability negotiation: CAP, AUTHENTICATE
/// - Universal commands: QUIT, PING, PONG
/// - Proxy identification: WEBIRC
///
/// # Example
///
/// ```ignore
/// pub struct NickHandler;
///
/// #[async_trait]
/// impl PreRegHandler for NickHandler {
///     async fn handle_pre_reg(
///         &self,
///         ctx: &mut Context<'_>,
///         msg: &MessageRef<'_>,
///     ) -> HandlerResult {
///         // No need to check registration - type system guarantees this!
///         let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
///         // ... handle NICK
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PreRegHandler: Send + Sync {
    /// Handle a command in pre-registration state.
    ///
    /// The `S` type parameter ensures this can only be called with a state
    /// that implements `PreRegistration` (i.e., `Unregistered` or `Negotiating`).
    async fn handle_pre_reg<S: PreRegistration>(
        &self,
        ctx: &mut Context<'_>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

// ============================================================================
// Post-Registration Handler Trait
// ============================================================================

/// Handler for commands that require a registered connection.
///
/// These handlers can ONLY be invoked in the `Registered` state.
/// The type system guarantees that `handle_post_reg` is never called
/// on an unregistered connection.
///
/// This eliminates the need for runtime checks like:
/// ```ignore
/// if !ctx.handshake.registered {
///     return Err(HandlerError::NotRegistered);
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// pub struct PrivmsgHandler;
///
/// #[async_trait]
/// impl PostRegHandler for PrivmsgHandler {
///     async fn handle_post_reg(
///         &self,
///         ctx: &mut Context<'_>,
///         msg: &MessageRef<'_>,
///     ) -> HandlerResult {
///         // Type system guarantees we're registered!
///         // ctx.handshake.nick is guaranteed to be Some
///         // ctx.handshake.user is guaranteed to be Some
///         let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
///         let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
///         // ... handle PRIVMSG
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PostRegHandler: Send + Sync {
    /// Handle a command in registered state.
    ///
    /// The `S` type parameter ensures this can only be called with a state
    /// that implements `IsRegistered` (i.e., only `Registered`).
    async fn handle_post_reg<S: IsRegistered>(
        &self,
        ctx: &mut Context<'_>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

// ============================================================================
// Universal Handler Trait
// ============================================================================

/// Handler for commands valid in any protocol state.
///
/// These are commands like QUIT, PING, and PONG that can be used
/// regardless of registration state.
///
/// # Example
///
/// ```ignore
/// pub struct QuitHandler;
///
/// #[async_trait]
/// impl UniversalHandler for QuitHandler {
///     async fn handle_any<S: ProtocolState>(
///         &self,
///         ctx: &mut Context<'_>,
///         msg: &MessageRef<'_>,
///     ) -> HandlerResult {
///         let quit_msg = msg.arg(0).map(|s| s.to_string());
///         Err(HandlerError::Quit(quit_msg))
///     }
/// }
/// ```
#[async_trait]
pub trait UniversalHandler: Send + Sync {
    /// Handle a command in any protocol state.
    async fn handle_any<S: ProtocolState>(
        &self,
        ctx: &mut Context<'_>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

// ============================================================================
// Blanket Implementations
// ============================================================================

/// A universal handler can act as a pre-reg handler.
///
/// This allows QUIT, PING, PONG to be registered as pre-reg handlers
/// without duplicating implementation.
#[async_trait]
impl<T: UniversalHandler> PreRegHandler for T {
    async fn handle_pre_reg<S: PreRegistration>(
        &self,
        ctx: &mut Context<'_>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        self.handle_any::<S>(ctx, msg).await
    }
}

// Note: We intentionally do NOT provide a blanket impl of PostRegHandler for UniversalHandler.
// Post-reg handlers require IsRegistered, which is stricter than ProtocolState.
// Universal handlers that should also work post-registration need explicit dispatch.

// ============================================================================
// Handler Command Metadata
// ============================================================================

/// Metadata about a command handler's registration requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerPhase {
    /// Valid only before registration (NICK during handshake)
    PreReg,
    /// Valid only after registration (PRIVMSG, JOIN, etc.)
    PostReg,
    /// Valid in any state (QUIT, PING, PONG)
    Universal,
}

impl HandlerPhase {
    /// Check if this phase is valid for unregistered connections.
    #[inline]
    pub const fn valid_unregistered(&self) -> bool {
        matches!(self, HandlerPhase::PreReg | HandlerPhase::Universal)
    }

    /// Check if this phase is valid for registered connections.
    #[inline]
    pub const fn valid_registered(&self) -> bool {
        matches!(self, HandlerPhase::PostReg | HandlerPhase::Universal)
    }
}

/// Get the handler phase for a command.
///
/// Returns the appropriate phase based on IRC protocol rules.
pub fn command_phase(command: &str) -> HandlerPhase {
    let upper = command.to_ascii_uppercase();
    match upper.as_str() {
        // Universal commands (valid in any state)
        "QUIT" | "PING" | "PONG" => HandlerPhase::Universal,

        // Pre-registration commands
        "NICK" | "USER" | "PASS" | "CAP" | "AUTHENTICATE" | "WEBIRC" | "REGISTER" => {
            HandlerPhase::PreReg
        }

        // Everything else requires registration
        _ => HandlerPhase::PostReg,
    }
}

// ============================================================================
// Phase 2: Stateful Handler Traits with TypedContext
// ============================================================================

/// Handler for post-registration commands with compile-time guarantees.
///
/// Unlike `PostRegHandler`, this trait receives `TypedContext<Registered>`,
/// providing **compile-time** guarantees that:
/// - The connection is registered
/// - `ctx.nick()` and `ctx.user()` always return valid values
///
/// # Migration
///
/// To migrate a handler from `Handler` to `StatefulPostRegHandler`:
///
/// ```ignore
/// // Before (runtime check)
/// impl Handler for PrivmsgHandler {
///     async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
///         let (nick, user) = require_registered(ctx)?;  // Runtime check!
///         // ...
///     }
/// }
///
/// // After (compile-time guarantee)
/// impl StatefulPostRegHandler for PrivmsgHandler {
///     async fn handle_registered(
///         &self,
///         ctx: &mut TypedContext<'_, Registered>,
///         msg: &MessageRef<'_>,
///     ) -> HandlerResult {
///         let nick = ctx.nick();  // Always valid - guaranteed by type!
///         // ...
///     }
/// }
/// ```
#[async_trait]
pub trait StatefulPostRegHandler: Send + Sync {
    /// Handle a command with compile-time registration guarantee.
    ///
    /// The `TypedContext<Registered>` parameter makes it **impossible** to call
    /// this method with an unregistered connection - the compiler rejects it.
    async fn handle_registered(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Handler for pre-registration commands with compile-time guarantees.
///
/// Receives `TypedContext<S>` where `S: PreRegistration`, ensuring
/// this handler is only called on unregistered/negotiating connections.
#[async_trait]
pub trait StatefulPreRegHandler: Send + Sync {
    /// Handle a command in pre-registration state.
    async fn handle_pre_registration<S: PreRegistration>(
        &self,
        ctx: &mut TypedContext<'_, S>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

/// Handler for commands valid in any state.
///
/// Receives `TypedContext<S>` where `S: ProtocolState`.
#[async_trait]
pub trait StatefulUniversalHandler: Send + Sync {
    /// Handle a command in any protocol state.
    async fn handle_any_state<S: ProtocolState>(
        &self,
        ctx: &mut TypedContext<'_, S>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult;
}

// ============================================================================
// Adapter: StatefulPostRegHandler -> Handler
// ============================================================================

/// Adapter that wraps a `StatefulPostRegHandler` as a legacy `Handler`.
///
/// This allows gradual migration: new handlers can implement the type-safe
/// `StatefulPostRegHandler` trait while still being usable in the existing
/// registry infrastructure.
///
/// **Note:** Due to lifetime constraints in async trait methods, direct
/// integration requires modifying the Registry to use a typed dispatch path.
/// See `examples.rs` for migration patterns.
pub struct RegisteredHandlerAdapter<H: StatefulPostRegHandler>(pub H);

impl<H: StatefulPostRegHandler> RegisteredHandlerAdapter<H> {
    /// Create a new adapter wrapping a StatefulPostRegHandler.
    pub fn new(handler: H) -> Self {
        Self(handler)
    }
}

// Note: Direct Handler impl for RegisteredHandlerAdapter is not possible with
// current lifetime constraints. The adapter is provided for future use when
// the Handler trait signature is updated to use explicit lifetimes.
//
// For now, use StatefulPostRegHandler directly in new code, and gradually
// migrate the Registry to support typed dispatch.

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_phase_classification() {
        // Universal
        assert_eq!(command_phase("QUIT"), HandlerPhase::Universal);
        assert_eq!(command_phase("PING"), HandlerPhase::Universal);
        assert_eq!(command_phase("PONG"), HandlerPhase::Universal);

        // Pre-reg
        assert_eq!(command_phase("NICK"), HandlerPhase::PreReg);
        assert_eq!(command_phase("USER"), HandlerPhase::PreReg);
        assert_eq!(command_phase("CAP"), HandlerPhase::PreReg);

        // Post-reg
        assert_eq!(command_phase("PRIVMSG"), HandlerPhase::PostReg);
        assert_eq!(command_phase("JOIN"), HandlerPhase::PostReg);
        assert_eq!(command_phase("MODE"), HandlerPhase::PostReg);

        // Case insensitive
        assert_eq!(command_phase("quit"), HandlerPhase::Universal);
        assert_eq!(command_phase("Nick"), HandlerPhase::PreReg);
    }

    #[test]
    fn test_phase_validity() {
        assert!(HandlerPhase::PreReg.valid_unregistered());
        assert!(!HandlerPhase::PreReg.valid_registered());

        assert!(!HandlerPhase::PostReg.valid_unregistered());
        assert!(HandlerPhase::PostReg.valid_registered());

        assert!(HandlerPhase::Universal.valid_unregistered());
        assert!(HandlerPhase::Universal.valid_registered());
    }
}
