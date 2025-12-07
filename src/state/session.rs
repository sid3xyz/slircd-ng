//! Session state types for true typestate enforcement (Innovation 1 Phase 3).
//!
//! This module defines the **data-carrying** state types that replace `HandshakeState`.
//! The key insight is that the state type itself holds all relevant data, and state
//! transitions consume the old state to produce a new one.
//!
//! ## Design Principles
//!
//! 1. **State types hold data** — not just markers with PhantomData
//! 2. **Transition consumes old state** — `try_register(self)` takes ownership
//! 3. **Guaranteed fields** — `RegisteredState.nick` is `String`, not `Option<String>`
//! 4. **No runtime flags** — no `registered: bool`, the TYPE is the state
//!
//! ## State Machine
//!
//! ```text
//! ┌─────────────────────┐     try_register()     ┌─────────────────────┐
//! │  UnregisteredState  │ ────────────────────▶  │   RegisteredState   │
//! │  nick: Option       │     (consumes self)    │   nick: String ✓    │
//! │  user: Option       │                        │   user: String ✓    │
//! └─────────────────────┘                        └─────────────────────┘
//! ```

use crate::handlers::{BatchState, SaslState};
use std::collections::HashSet;
use std::time::Instant;

// ============================================================================
// UnregisteredState — Pre-registration connection state
// ============================================================================

/// State for connections that have not yet completed registration.
///
/// Pre-registration commands (NICK, USER, CAP, PASS, WEBIRC, AUTHENTICATE)
/// operate on this state. Nick and user are `Option` because they haven't
/// been provided yet.
#[derive(Debug, Default)]
pub struct UnregisteredState {
    /// Nick provided by NICK command.
    pub nick: Option<String>,
    /// Username provided by USER command.
    pub user: Option<String>,
    /// Realname provided by USER command.
    pub realname: Option<String>,
    /// Whether CAP negotiation is in progress.
    pub cap_negotiating: bool,
    /// CAP protocol version (301 or 302).
    pub cap_version: u32,
    /// Capabilities enabled by this client.
    pub capabilities: HashSet<String>,
    /// SASL authentication state.
    pub sasl_state: SaslState,
    /// Buffer for accumulating chunked SASL data (for large payloads).
    pub sasl_buffer: String,
    /// Account name if SASL authenticated.
    pub account: Option<String>,
    /// Whether this is a TLS connection.
    pub is_tls: bool,
    /// TLS client certificate fingerprint (SHA-256, hex-encoded).
    pub certfp: Option<String>,
    /// Whether WEBIRC was used to set client info.
    pub webirc_used: bool,
    /// Real IP address from WEBIRC (overrides connection IP).
    pub webirc_ip: Option<String>,
    /// Real hostname from WEBIRC (overrides reverse DNS).
    pub webirc_host: Option<String>,
    /// Password received via PASS command.
    pub pass_received: Option<String>,
}

impl UnregisteredState {
    /// Create a new unregistered state.
    pub fn new(is_tls: bool, certfp: Option<String>) -> Self {
        Self {
            is_tls,
            certfp,
            ..Default::default()
        }
    }

    /// Check if registration requirements are met.
    ///
    /// Requirements:
    /// - NICK has been provided
    /// - USER has been provided
    /// - CAP negotiation is not in progress (if started)
    pub fn can_register(&self) -> bool {
        self.nick.is_some() && self.user.is_some() && !self.cap_negotiating
    }

    /// Attempt to transition to RegisteredState.
    ///
    /// This **consumes** self. If registration requirements are not met,
    /// returns `Err(self)` so the caller can continue using the state.
    ///
    /// This is the "Parse, Don't Validate" pattern — we parse the unregistered
    /// state into a registered state once, rather than checking a flag repeatedly.
    pub fn try_register(self) -> Result<RegisteredState, Self> {
        match (&self.nick, &self.user) {
            (Some(nick), Some(user)) if !self.cap_negotiating => {
                Ok(RegisteredState {
                    nick: nick.clone(),
                    user: user.clone(),
                    realname: self.realname.unwrap_or_default(),
                    capabilities: self.capabilities,
                    account: self.account,
                    is_tls: self.is_tls,
                    certfp: self.certfp,
                    // Post-registration state starts fresh
                    failed_oper_attempts: 0,
                    last_oper_attempt: None,
                    active_batch: None,
                    active_batch_ref: None,
                })
            }
            _ => Err(self),
        }
    }
}

// ============================================================================
// RegisteredState — Post-registration connection state
// ============================================================================

/// State for fully registered connections.
///
/// Post-registration commands (PRIVMSG, JOIN, MODE, etc.) operate on this state.
/// Nick and user are **guaranteed** to be present — they are `String`, not `Option`.
///
/// ## Compile-Time Guarantees
///
/// When a handler receives `Context<'_, RegisteredState>`:
/// - `ctx.state.nick` is always valid (no unwrap needed)
/// - `ctx.state.user` is always valid (no unwrap needed)
/// - The connection has completed the full registration handshake
#[derive(Debug)]
pub struct RegisteredState {
    /// Nick — guaranteed present after registration.
    pub nick: String,
    /// Username — guaranteed present after registration.
    pub user: String,
    /// Realname (may be empty but is always a valid String).
    pub realname: String,
    /// Capabilities enabled by this client.
    pub capabilities: HashSet<String>,
    /// Account name if authenticated (SASL or services).
    pub account: Option<String>,
    /// Whether this is a TLS connection.
    pub is_tls: bool,
    /// TLS client certificate fingerprint.
    pub certfp: Option<String>,
    /// Failed OPER attempts counter (brute-force protection).
    pub failed_oper_attempts: u8,
    /// Timestamp of last OPER attempt (for rate limiting).
    pub last_oper_attempt: Option<Instant>,
    /// Active batch state for client-to-server batches (e.g., draft/multiline).
    pub active_batch: Option<BatchState>,
    /// Reference tag for the active batch.
    pub active_batch_ref: Option<String>,
}

impl RegisteredState {
    /// Check if a capability is enabled.
    #[inline]
    pub fn has_cap(&self, cap: &str) -> bool {
        self.capabilities.contains(cap)
    }

    /// Get account name for message tags.
    #[inline]
    pub fn account_tag(&self) -> Option<&str> {
        self.account.as_deref()
    }
}

// ============================================================================
// ConnectionState enum — For the connection loop state machine
// ============================================================================

/// State machine for connection lifecycle.
///
/// Used by the connection loop to track which phase the connection is in.
/// This replaces the `registered: bool` flag with an explicit enum.
pub enum ConnectionState {
    /// Connection is in pre-registration phase.
    Unregistered(UnregisteredState),
    /// Connection has completed registration.
    Registered(RegisteredState),
}

impl ConnectionState {
    /// Create a new connection in unregistered state.
    pub fn new(is_tls: bool, certfp: Option<String>) -> Self {
        Self::Unregistered(UnregisteredState::new(is_tls, certfp))
    }

    /// Check if this connection is registered.
    #[inline]
    pub fn is_registered(&self) -> bool {
        matches!(self, Self::Registered(_))
    }

    /// Get nick if available (for error messages, logging).
    pub fn nick(&self) -> Option<&str> {
        match self {
            Self::Unregistered(s) => s.nick.as_deref(),
            Self::Registered(s) => Some(&s.nick),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unregistered_cannot_register_without_nick() {
        let state = UnregisteredState::default();
        assert!(!state.can_register());
        assert!(state.try_register().is_err());
    }

    #[test]
    fn test_unregistered_cannot_register_without_user() {
        let mut state = UnregisteredState::default();
        state.nick = Some("test".to_string());
        assert!(!state.can_register());
        assert!(state.try_register().is_err());
    }

    #[test]
    fn test_unregistered_cannot_register_during_cap_negotiation() {
        let mut state = UnregisteredState::default();
        state.nick = Some("test".to_string());
        state.user = Some("testuser".to_string());
        state.cap_negotiating = true;
        assert!(!state.can_register());
        assert!(state.try_register().is_err());
    }

    #[test]
    fn test_successful_registration() {
        let mut state = UnregisteredState::default();
        state.nick = Some("test".to_string());
        state.user = Some("testuser".to_string());
        state.realname = Some("Test User".to_string());
        state.capabilities.insert("echo-message".to_string());
        state.account = Some("testaccount".to_string());

        assert!(state.can_register());

        let registered = state.try_register().expect("should register");
        assert_eq!(registered.nick, "test");
        assert_eq!(registered.user, "testuser");
        assert_eq!(registered.realname, "Test User");
        assert!(registered.capabilities.contains("echo-message"));
        assert_eq!(registered.account, Some("testaccount".to_string()));
    }

    #[test]
    fn test_registered_has_cap() {
        let state = RegisteredState {
            nick: "test".to_string(),
            user: "testuser".to_string(),
            realname: String::new(),
            capabilities: ["echo-message".to_string()].into_iter().collect(),
            account: None,
            is_tls: false,
            certfp: None,
            failed_oper_attempts: 0,
            last_oper_attempt: None,
            active_batch: None,
            active_batch_ref: None,
        };

        assert!(state.has_cap("echo-message"));
        assert!(!state.has_cap("server-time"));
    }
}
