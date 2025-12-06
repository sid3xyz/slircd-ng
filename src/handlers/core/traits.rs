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
//! ## Migration Path
//!
//! Existing handlers implement the `Handler` trait. To migrate:
//!
//! 1. For pre-reg handlers: implement `PreRegHandler` instead of `Handler`
//! 2. For post-reg handlers: implement `PostRegHandler` instead of `Handler`
//! 3. Remove the runtime `if !ctx.handshake.registered` check
//!
//! The registry will dispatch to the appropriate trait based on connection state.

// Foundation code - will be used in subsequent phases
#![allow(dead_code)]

use super::context::{Context, HandlerResult};
use crate::state::{IsRegistered, PreRegistration, ProtocolState};
use async_trait::async_trait;
use slirc_proto::MessageRef;

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
