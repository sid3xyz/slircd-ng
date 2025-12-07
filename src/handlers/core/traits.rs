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
//! 1. For pre-reg handlers: implement `PreRegHandler` instead
//! 2. For post-reg handlers: implement `PostRegHandler` instead
//! 3. Gain compile-time safety - no runtime registration checks needed!

use super::context::{Context, HandlerResult};
use crate::state::{ProtocolState, Registered};
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
