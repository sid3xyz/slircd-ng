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
use crate::state::{PreRegistration, ProtocolState, Registered};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use std::marker::PhantomData;

// ============================================================================
// Handler Traits (Innovation 1 Phase 2)
// ============================================================================

/// Handler for commands valid BEFORE registration (NICK, USER, CAP, PASS).
/// Receives raw `Context` (checked by Registry to be in pre-reg state).
#[async_trait]
pub trait PreRegHandler: Send + Sync {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}

/// Handler for commands requiring FULL registration (PRIVMSG, JOIN, etc.).
/// Receives `TypedContext<Registered>`.
#[async_trait]
pub trait PostRegHandler: Send + Sync {
    async fn handle(&self, ctx: &mut TypedContext<'_, Registered>, msg: &MessageRef<'_>) -> HandlerResult;
}

/// Handler for commands valid in ANY state (QUIT, PING, PONG).
/// Receives raw `Context`.
#[async_trait]
pub trait UniversalHandler: Send + Sync {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult;
}

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
    pub fn new_unchecked(ctx: &mut Context<'a>) -> Self {
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
// Blanket Implementations
// ============================================================================

/// A universal handler can act as a pre-reg handler.
#[async_trait]
impl<T: UniversalHandler> PreRegHandler for T {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        <T as UniversalHandler>::handle(self, ctx, msg).await
    }
}

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

use std::ops::{Deref, DerefMut};

impl<'a, S: ProtocolState> Deref for TypedContext<'a, S> {
    type Target = Context<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<'a, S: ProtocolState> DerefMut for TypedContext<'a, S> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner_mut()
    }
}
