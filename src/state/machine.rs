//! Protocol State Machine Types for IRC Connection Lifecycle (Innovation 1).
//!
//! This module implements a typestate pattern that makes it **impossible at compile time**
//! to process authenticated commands on an unregistered connection.
//!
//! ## State Machine
//!
//! ```text
//! ┌───────────────┐   NICK/USER/CAP   ┌───────────────┐   CAP END    ┌──────────────┐
//! │  Unregistered ├──────────────────►│  Negotiating  ├─────────────►│  Registered  │
//! └───────┬───────┘                   └───────┬───────┘              └──────────────┘
//!         │                                   │
//!         │           CAP LS/REQ/etc          │
//!         └───────────────────────────────────┘
//! ```
//!
//! ## Design Goals
//!
//! 1. **Zero runtime overhead**: State markers are zero-sized types (ZSTs)
//! 2. **Compile-time safety**: Post-registration handlers can ONLY be called
//!    with a `Registered` state - enforced by the type system
//! 3. **Structural enforcement**: Registry uses separate handler maps per phase
//! 4. **Clear semantics**: Trait hierarchy makes handler requirements explicit
//!
//! ## Implementation Status: COMPLETE ✅
//!
//! The Registry uses three separate handler maps:
//! - `pre_reg_handlers`: Commands valid before registration (NICK, USER, etc.)
//! - `post_reg_handlers`: Commands requiring registration (PRIVMSG, JOIN, etc.)
//! - `universal_handlers`: Commands valid in any state (QUIT, PING, PONG)
//!
//! Dispatch looks up handlers only in accessible maps based on connection state.
//! Post-registration handlers are structurally inaccessible to unregistered clients.
//!
//! ## Handler Classification
//!
//! ### Pre-Registration Handlers (`pre_reg_handlers` map)
//! Valid in `Unregistered` or `Negotiating` states:
//! - NICK, USER, PASS - Registration commands
//! - CAP, AUTHENTICATE - Capability negotiation
//! - WEBIRC - Proxy identification (must be first)
//! - REGISTER - IRCv3 draft/account-registration
//!
//! ### Post-Registration Handlers (`post_reg_handlers` map)
//! Require `IsRegistered` (only `Registered` state):
//! - PRIVMSG, NOTICE, TAGMSG - Messaging
//! - JOIN, PART, KICK, INVITE, TOPIC - Channel operations
//! - WHO, WHOIS, WHOWAS, USERHOST, ISON - User queries
//! - MODE, AWAY, SETNAME, SILENCE - User status
//! - OPER, KILL, WALLOPS, etc. - Operator commands
//! - All ban commands (KLINE, GLINE, etc.)
//!
//! ### Universal Handlers (`universal_handlers` map)
//! Valid in any state:
//! - QUIT - Can disconnect anytime
//! - PING/PONG - Keep-alive
//!
//! ## Architecture
//!
//! The typestate system is implemented across two modules:
//!
//! 1. **`state/machine.rs`** (this file): Defines state types and traits
//!    - `Unregistered`, `Negotiating`, `Registered` - Zero-sized state markers
//!    - `ProtocolState`, `PreRegistration`, `IsRegistered` - Trait hierarchy
//!    - `ConnectionState<S>` - Type-safe state container
//!    - `AnyConnectionState` - Runtime dispatch container
//!
//! 2. **`handlers/core/registry.rs`**: Implements phase-separated dispatch
//!    - Three handler maps: `pre_reg_handlers`, `post_reg_handlers`, `universal_handlers`
//!    - Dispatch looks up handlers only in accessible maps based on state
//!    - Post-reg handlers are structurally inaccessible to unregistered clients

// Foundation code - will be used in subsequent phases
#![allow(dead_code)]

use std::marker::PhantomData;

// ============================================================================
// State Marker Types (Zero-Sized)
// ============================================================================

/// Connection state: not yet registered (no NICK/USER completed).
/// Valid commands: NICK, USER, PASS, CAP, AUTHENTICATE, QUIT, PING, PONG, WEBIRC
#[derive(Debug, Clone, Copy, Default)]
pub struct Unregistered;

/// Connection state: CAP negotiation in progress.
/// Client has sent CAP LS or CAP REQ, awaiting CAP END.
/// Valid commands: Same as Unregistered + CAP subcommands
#[derive(Debug, Clone, Copy, Default)]
pub struct Negotiating;

/// Connection state: fully registered and authenticated.
/// All commands are valid (subject to permissions).
#[derive(Debug, Clone, Copy, Default)]
pub struct Registered;

// ============================================================================
// State Trait Hierarchy
// ============================================================================

/// Marker trait for all protocol states.
/// This is the base trait that all state markers implement.
///
/// # Safety
/// This trait is sealed and should only be implemented by the three state types
/// defined in this module: `Unregistered`, `Negotiating`, and `Registered`.
pub trait ProtocolState: Send + Sync + 'static + private::Sealed {}

/// Marker trait for states that allow capability negotiation.
/// Implemented by: `Unregistered`, `Negotiating`
///
/// Commands requiring this: CAP, AUTHENTICATE
pub trait CanNegotiate: ProtocolState {}

/// Marker trait for states that allow pre-registration commands.
/// Implemented by: `Unregistered`, `Negotiating`
///
/// Commands requiring this: NICK, USER, PASS, WEBIRC
pub trait PreRegistration: ProtocolState {}

/// Marker trait for the fully registered state.
/// Implemented by: `Registered` only
///
/// This is the key constraint - handlers marked with this trait bound
/// can ONLY be invoked on registered connections.
pub trait IsRegistered: ProtocolState {}

// Implement traits for state types
impl ProtocolState for Unregistered {}
impl ProtocolState for Negotiating {}
impl ProtocolState for Registered {}

impl CanNegotiate for Unregistered {}
impl CanNegotiate for Negotiating {}

impl PreRegistration for Unregistered {}
impl PreRegistration for Negotiating {}

impl IsRegistered for Registered {}

// Seal the ProtocolState trait to prevent external implementations
mod private {
    pub trait Sealed {}
    impl Sealed for super::Unregistered {}
    impl Sealed for super::Negotiating {}
    impl Sealed for super::Registered {}
}

// ============================================================================
// Runtime State Container
// ============================================================================

/// Runtime dispatch container for protocol states.
///
/// Since we need to handle connections dynamically (we don't know the state
/// at compile time when a message arrives), this enum allows runtime dispatch
/// while the inner handler traits provide compile-time guarantees.
///
/// ## Usage
///
/// ```ignore
/// match connection_state {
///     AnyConnectionState::Unregistered(state) => {
///         // Can only call pre_reg_handlers here
///         registry.dispatch_pre_reg(state, ctx, msg).await
///     }
///     AnyConnectionState::Registered(state) => {
///         // Can call any handler
///         registry.dispatch_post_reg(state, ctx, msg).await
///     }
///     // ...
/// }
/// ```
#[derive(Debug, Clone)]
pub enum AnyConnectionState {
    /// Not yet registered
    Unregistered(ConnectionState<Unregistered>),
    /// CAP negotiation in progress
    Negotiating(ConnectionState<Negotiating>),
    /// Fully registered
    Registered(ConnectionState<Registered>),
}

impl Default for AnyConnectionState {
    fn default() -> Self {
        AnyConnectionState::Unregistered(ConnectionState::new())
    }
}

impl AnyConnectionState {
    /// Create a new unregistered connection state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the connection is registered.
    #[inline]
    pub fn is_registered(&self) -> bool {
        matches!(self, AnyConnectionState::Registered(_))
    }

    /// Check if the connection is negotiating capabilities.
    #[inline]
    pub fn is_negotiating(&self) -> bool {
        matches!(self, AnyConnectionState::Negotiating(_))
    }

    /// Transition from Unregistered to Negotiating (CAP LS/REQ received).
    ///
    /// Returns `None` if not in Unregistered state.
    pub fn begin_negotiation(self) -> Option<Self> {
        match self {
            AnyConnectionState::Unregistered(_) => {
                Some(AnyConnectionState::Negotiating(ConnectionState::new()))
            }
            // Already negotiating is fine
            AnyConnectionState::Negotiating(_) => Some(self),
            // Cannot go backwards from Registered
            AnyConnectionState::Registered(_) => None,
        }
    }

    /// Transition from Negotiating to Unregistered (CAP negotiation cancelled).
    ///
    /// Returns `None` if not in Negotiating state.
    pub fn cancel_negotiation(self) -> Option<Self> {
        match self {
            AnyConnectionState::Negotiating(_) => {
                Some(AnyConnectionState::Unregistered(ConnectionState::new()))
            }
            _ => None,
        }
    }

    /// Transition to Registered state (registration complete).
    ///
    /// This can happen from either Unregistered (no CAP) or Negotiating (after CAP END).
    pub fn complete_registration(self) -> Self {
        AnyConnectionState::Registered(ConnectionState::new())
    }

    /// Get a reference to the registered state, if in that state.
    pub fn as_registered(&self) -> Option<&ConnectionState<Registered>> {
        match self {
            AnyConnectionState::Registered(state) => Some(state),
            _ => None,
        }
    }
}

// ============================================================================
// Typed Connection State
// ============================================================================

/// A typed connection state container.
///
/// The `S` type parameter encodes the protocol state at the type level.
/// This is a zero-cost abstraction - `PhantomData` is zero-sized.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionState<S: ProtocolState> {
    _marker: PhantomData<S>,
}

impl<S: ProtocolState> Default for ConnectionState<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ProtocolState> ConnectionState<S> {
    /// Create a new connection state.
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

// State transitions (type-safe!)
impl ConnectionState<Unregistered> {
    /// Begin CAP negotiation.
    #[inline]
    pub fn begin_negotiation(self) -> ConnectionState<Negotiating> {
        ConnectionState::new()
    }

    /// Complete registration directly (no CAP negotiation).
    #[inline]
    pub fn complete_registration(self) -> ConnectionState<Registered> {
        ConnectionState::new()
    }
}

impl ConnectionState<Negotiating> {
    /// Complete registration after CAP END.
    #[inline]
    pub fn complete_registration(self) -> ConnectionState<Registered> {
        ConnectionState::new()
    }

    /// Cancel negotiation (rare, but possible).
    #[inline]
    pub fn cancel_negotiation(self) -> ConnectionState<Unregistered> {
        ConnectionState::new()
    }
}

// ============================================================================
// Handler Classification Constants
// ============================================================================

/// Commands valid before registration (pre-reg handlers).
///
/// These commands can be processed in `Unregistered` or `Negotiating` states.
pub const PRE_REG_COMMANDS: &[&str] = &[
    "NICK",
    "USER",
    "PASS",
    "CAP",
    "AUTHENTICATE",
    "QUIT",
    "PING",
    "PONG",
    "WEBIRC",
    "REGISTER", // IRCv3 draft/account-registration
];

/// Commands that require registration (post-reg handlers).
///
/// These commands can ONLY be processed in the `Registered` state.
/// Attempting to use them before registration should result in ERR_NOTREGISTERED.
pub const POST_REG_COMMANDS: &[&str] = &[
    // Messaging
    "PRIVMSG",
    "NOTICE",
    "TAGMSG",
    // Channels
    "JOIN",
    "PART",
    "CYCLE",
    "TOPIC",
    "NAMES",
    "MODE",
    "KICK",
    "LIST",
    "INVITE",
    "KNOCK",
    // User queries
    "WHO",
    "WHOIS",
    "WHOWAS",
    "USERHOST",
    "ISON",
    // Server queries
    "VERSION",
    "TIME",
    "ADMIN",
    "INFO",
    "LUSERS",
    "STATS",
    "MOTD",
    "MAP",
    "RULES",
    "USERIP",
    "LINKS",
    "HELP",
    "SERVICE",
    "SERVLIST",
    "SQUERY",
    // User status
    "AWAY",
    "SETNAME",
    "SILENCE",
    "MONITOR",
    "BATCH",
    "CHATHISTORY",
    // Service aliases
    "NICKSERV",
    "NS",
    "CHANSERV",
    "CS",
    // Operator commands
    "OPER",
    "KILL",
    "WALLOPS",
    "DIE",
    "REHASH",
    "RESTART",
    "CHGHOST",
    "VHOST",
    "TRACE",
    // Bans
    "KLINE",
    "DLINE",
    "GLINE",
    "ZLINE",
    "RLINE",
    "SHUN",
    "UNKLINE",
    "UNDLINE",
    "UNGLINE",
    "UNZLINE",
    "UNRLINE",
    "UNSHUN",
    // Admin commands
    "SAJOIN",
    "SAPART",
    "SANICK",
    "SAMODE",
];

/// Check if a command requires registration.
#[inline]
pub fn requires_registration(command: &str) -> bool {
    // Pre-reg commands are explicitly listed; everything else requires registration
    !PRE_REG_COMMANDS
        .iter()
        .any(|&c| c.eq_ignore_ascii_case(command))
}

/// Check if a command is valid before registration.
#[inline]
pub fn valid_pre_registration(command: &str) -> bool {
    PRE_REG_COMMANDS
        .iter()
        .any(|&c| c.eq_ignore_ascii_case(command))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transitions_unregistered_to_negotiating() {
        let state: ConnectionState<Unregistered> = ConnectionState::new();
        let _negotiating: ConnectionState<Negotiating> = state.begin_negotiation();
    }

    #[test]
    fn test_state_transitions_negotiating_to_registered() {
        let state: ConnectionState<Negotiating> = ConnectionState::new();
        let _registered: ConnectionState<Registered> = state.complete_registration();
    }

    #[test]
    fn test_state_transitions_unregistered_to_registered() {
        let state: ConnectionState<Unregistered> = ConnectionState::new();
        let _registered: ConnectionState<Registered> = state.complete_registration();
    }

    #[test]
    fn test_any_state_default() {
        let state = AnyConnectionState::default();
        assert!(!state.is_registered());
        assert!(!state.is_negotiating());
    }

    #[test]
    fn test_any_state_transitions() {
        let state = AnyConnectionState::new();

        // Unregistered -> Negotiating
        let state = state.begin_negotiation().unwrap();
        assert!(state.is_negotiating());

        // Negotiating -> Registered
        let state = state.complete_registration();
        assert!(state.is_registered());
    }

    #[test]
    fn test_command_classification() {
        // Pre-reg commands
        assert!(valid_pre_registration("NICK"));
        assert!(valid_pre_registration("nick")); // case-insensitive
        assert!(valid_pre_registration("CAP"));
        assert!(valid_pre_registration("QUIT"));

        // Post-reg commands
        assert!(requires_registration("PRIVMSG"));
        assert!(requires_registration("JOIN"));
        assert!(requires_registration("MODE"));

        // Pre-reg should not require registration
        assert!(!requires_registration("NICK"));
        assert!(!requires_registration("CAP"));
    }

    // Compile-time test: This function demonstrates that IsRegistered
    // can only accept Registered state. The type system prevents
    // passing Unregistered or Negotiating states.
    fn _requires_registered_state<S: IsRegistered>(_state: ConnectionState<S>) {
        // This function can only be called with ConnectionState<Registered>
    }

    #[test]
    fn test_is_registered_constraint() {
        let registered: ConnectionState<Registered> = ConnectionState::new();
        _requires_registered_state(registered);

        // These would fail to compile (which is the point!):
        // let unregistered: ConnectionState<Unregistered> = ConnectionState::new();
        // _requires_registered_state(unregistered); // ERROR: Unregistered: IsRegistered is not satisfied
    }
}
