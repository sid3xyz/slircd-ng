//! Session state types for typestate enforcement (Innovation 1).
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
use crate::state::client::{DeviceId, SessionId};
use slirc_crdt::clock::ServerId;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use uuid::Uuid;

// ============================================================================
// SaslAccess trait — SASL state access for universal AUTHENTICATE handler
// ============================================================================

/// Trait for accessing SASL authentication state.
///
/// This allows the AUTHENTICATE handler to work with both pre-registration
/// and post-registration connections.
pub trait SaslAccess {
    /// Get the current SASL state.
    fn sasl_state(&self) -> &SaslState;
    /// Set the SASL state.
    fn set_sasl_state(&mut self, state: SaslState);
    /// Get the SASL buffer for accumulating chunked data.
    fn sasl_buffer(&self) -> &str;
    /// Get mutable access to the SASL buffer.
    fn sasl_buffer_mut(&mut self) -> &mut String;
    /// Set the account name.
    fn set_account(&mut self, account: Option<String>);
}

// ============================================================================
// SessionState trait — Unified interface for universal handlers
// ============================================================================

/// Common interface for both UnregisteredState and RegisteredState.
///
/// This trait allows universal handlers (QUIT, PING, PONG, NICK, CAP) to work
/// with both state types without code duplication. Each method provides access
/// to fields that exist in both states, with appropriate semantics.
pub trait SessionState: Send {
    /// Get the nick, if set. Always `Some` for RegisteredState.
    fn nick(&self) -> Option<&str>;

    /// Get the nick or "*" for error messages.
    fn nick_or_star(&self) -> &str {
        self.nick().unwrap_or("*")
    }

    /// Set the nick (during registration or NICK change).
    fn set_nick(&mut self, nick: String);

    /// Whether the connection is registered (type-level truth).
    fn is_registered(&self) -> bool;

    /// Get the session ID for this connection.
    ///
    /// For registered connections, returns the unique session UUID.
    /// For unregistered/server connections, returns a nil UUID.
    fn session_id(&self) -> SessionId {
        Uuid::nil()
    }

    /// Set the device ID for this session (for bouncer/multiclient).
    ///
    /// Default implementation does nothing (for unregistered/server connections).
    fn set_device_id(&mut self, _device_id: Option<DeviceId>) {}

    /// Get enabled capabilities.
    fn capabilities(&self) -> &HashSet<String>;

    /// Get mutable capabilities (for CAP REQ).
    fn capabilities_mut(&mut self) -> &mut HashSet<String>;

    /// Set CAP negotiation state.
    fn set_cap_negotiating(&mut self, negotiating: bool);

    /// Set CAP protocol version.
    fn set_cap_version(&mut self, version: u32);

    /// Whether this is a TLS connection.
    fn is_tls(&self) -> bool;

    /// Get TLS certificate fingerprint.
    fn certfp(&self) -> Option<&str>;

    /// Get mutable active batch state.
    fn active_batch_mut(&mut self) -> &mut Option<BatchState>;

    /// Get active batch reference tag.
    fn active_batch_ref(&self) -> Option<&str>;

    /// Whether this is a server connection.
    fn is_server(&self) -> bool;

    /// Get batch routing decision (Server only).
    fn batch_routing(&self) -> Option<&BatchRouting> {
        None
    }
}

// ============================================================================
// ServerState — Server-to-server connection state
// ============================================================================

/// State for a registered server-to-server connection.
#[derive(Debug)]
pub struct ServerState {
    /// Server name.
    pub name: String,
    /// Server ID (SID).
    pub sid: String,
    /// Server info string.
    pub info: String,
    /// Hop count.
    pub hopcount: u32,
    /// Capabilities enabled by this server.
    pub capabilities: HashSet<String>,
    /// Whether this is a TLS connection.
    pub is_tls: bool,
    /// Active batch state for server-to-server batches.
    pub active_batch: Option<BatchState>,
    /// Reference tag for the active batch.
    pub active_batch_ref: Option<String>,
    /// Routing decision for the active batch.
    pub batch_routing: Option<BatchRouting>,
}

/// Routing decision for a server batch.
#[derive(Debug, Clone)]
pub enum BatchRouting {
    /// Broadcast to all peers (except source).
    Broadcast,
    /// Route to a specific server.
    Routed(ServerId),
    /// Route to a local user.
    Local(String),
    /// Do not relay.
    None,
}

impl SessionState for ServerState {
    fn nick(&self) -> Option<&str> {
        None
    }

    fn set_nick(&mut self, _nick: String) {}

    fn is_registered(&self) -> bool {
        true
    }

    fn is_server(&self) -> bool {
        true
    }

    fn capabilities(&self) -> &HashSet<String> {
        &self.capabilities
    }

    fn capabilities_mut(&mut self) -> &mut HashSet<String> {
        &mut self.capabilities
    }

    fn set_cap_negotiating(&mut self, _negotiating: bool) {}

    fn set_cap_version(&mut self, _version: u32) {}

    fn is_tls(&self) -> bool {
        self.is_tls
    }

    fn certfp(&self) -> Option<&str> {
        None
    }

    fn active_batch_mut(&mut self) -> &mut Option<BatchState> {
        &mut self.active_batch
    }

    fn active_batch_ref(&self) -> Option<&str> {
        self.active_batch_ref.as_deref()
    }

    fn batch_routing(&self) -> Option<&BatchRouting> {
        self.batch_routing.as_ref()
    }
}

/// ServerState doesn't do SASL - these are no-ops with panic paths that should never execute.
impl SaslAccess for ServerState {
    fn sasl_state(&self) -> &SaslState {
        // Servers don't authenticate via SASL - this should never be called
        static NONE: SaslState = SaslState::None;
        &NONE
    }

    fn set_sasl_state(&mut self, _state: SaslState) {
        // No-op for servers
    }

    fn sasl_buffer(&self) -> &str {
        ""
    }

    fn sasl_buffer_mut(&mut self) -> &mut String {
        // This should never be called for servers
        panic!("ServerState does not support SASL buffer access")
    }

    fn set_account(&mut self, _account: Option<String>) {
        // No-op for servers
    }
}

// ============================================================================
// ReattachInfo — Bouncer session reattachment data
// ============================================================================

/// Information for reattaching a bouncer session.
///
/// This is populated by the SASL handler when a client attaches to an
/// existing always-on client session, and consumed by the autoreplay
/// logic after registration completes.
#[derive(Debug, Clone)]
pub struct ReattachInfo {
    /// Account name that was authenticated.
    pub account: String,
    /// Device ID from the SASL username (account@device).
    pub device_id: Option<String>,
    /// Channels the client was in (name -> membership info).
    pub channels: Vec<(String, crate::state::ChannelMembership)>,
    /// When to replay history from (typically client's last_seen).
    pub replay_since: Option<chrono::DateTime<chrono::Utc>>,
}

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
    /// Active batch state for client-to-server batches (e.g., draft/multiline).
    pub active_batch: Option<BatchState>,
    /// Reference tag for the active batch.
    pub active_batch_ref: Option<String>,
    /// Whether this connection is performing a server-to-server handshake.
    pub is_server_handshake: bool,
    /// Server name (if SERVER command received).
    pub server_name: Option<String>,
    /// Server ID (if SERVER command received).
    pub server_sid: Option<String>,
    /// Server info (if SERVER command received).
    pub server_info: Option<String>,
    /// Server hop count (if SERVER command received).
    pub server_hopcount: u32,
    /// Server capabilities (if CAPAB command received).
    pub server_capab: Option<Vec<String>>,
    /// Server version info (if SVINFO command received).
    pub server_svinfo: Option<(u32, u32, u32, u64)>,
    /// Data for initiating a server connection.
    pub initiator_data: Option<InitiatorData>,
    /// Reattach info for bouncer session (set by SASL, carried to RegisteredState).
    pub reattach_info: Option<ReattachInfo>,
}

/// Data for initiating a server connection.
#[derive(Debug, Clone)]
pub struct InitiatorData {
    /// Password to send in PASS command.
    pub remote_password: String,
    /// Expected remote SID (optional).
    pub remote_sid: Option<String>,
}

impl SessionState for UnregisteredState {
    fn nick(&self) -> Option<&str> {
        self.nick.as_deref()
    }

    fn set_nick(&mut self, nick: String) {
        self.nick = Some(nick);
    }

    fn is_registered(&self) -> bool {
        false
    }

    fn is_server(&self) -> bool {
        false
    }

    fn capabilities(&self) -> &HashSet<String> {
        &self.capabilities
    }

    fn capabilities_mut(&mut self) -> &mut HashSet<String> {
        &mut self.capabilities
    }

    fn set_cap_negotiating(&mut self, negotiating: bool) {
        self.cap_negotiating = negotiating;
    }

    fn set_cap_version(&mut self, version: u32) {
        self.cap_version = version;
    }

    fn is_tls(&self) -> bool {
        self.is_tls
    }

    fn certfp(&self) -> Option<&str> {
        self.certfp.as_deref()
    }

    fn active_batch_mut(&mut self) -> &mut Option<BatchState> {
        &mut self.active_batch
    }

    fn active_batch_ref(&self) -> Option<&str> {
        self.active_batch_ref.as_deref()
    }
}

impl SaslAccess for UnregisteredState {
    fn sasl_state(&self) -> &SaslState {
        &self.sasl_state
    }

    fn set_sasl_state(&mut self, state: SaslState) {
        self.sasl_state = state;
    }

    fn sasl_buffer(&self) -> &str {
        &self.sasl_buffer
    }

    fn sasl_buffer_mut(&mut self) -> &mut String {
        &mut self.sasl_buffer
    }

    fn set_account(&mut self, account: Option<String>) {
        self.account = account;
    }
}

impl UnregisteredState {
    /// Check if registration requirements are met.
    ///
    /// Requirements:
    /// - NICK has been provided
    /// - USER has been provided
    /// - CAP negotiation is not in progress (if started)
    pub fn can_register(&self) -> bool {
        self.nick.is_some() && self.user.is_some() && !self.cap_negotiating
    }

    /// Check if server registration requirements are met.
    pub fn can_register_server(&self) -> bool {
        self.is_server_handshake
            && self.server_name.is_some()
            && self.server_sid.is_some()
            && self.server_capab.is_some()
            && self.server_svinfo.is_some()
    }

    /// Attempt to transition to ServerState.
    #[allow(clippy::result_large_err)]
    pub fn try_register_server(self) -> Result<ServerState, Self> {
        // Use pattern matching to avoid unwrap() - destructure all required fields at once
        match (
            self.is_server_handshake,
            &self.server_name,
            &self.server_sid,
            &self.server_capab,
            self.server_svinfo.is_some(),
        ) {
            (true, Some(name), Some(sid), Some(capab), true) => {
                let capabilities = capab.iter().cloned().collect();

                Ok(ServerState {
                    name: name.clone(),
                    sid: sid.clone(),
                    info: self.server_info.unwrap_or_default(),
                    hopcount: self.server_hopcount,
                    capabilities,
                    is_tls: self.is_tls,
                    active_batch: None,
                    active_batch_ref: None,
                    batch_routing: None,
                })
            }
            _ => Err(self),
        }
    }

    /// Attempt to transition to RegisteredState.
    ///
    /// This **consumes** self. If registration requirements are not met,
    /// returns `Err(self)` so the caller can continue using the state.
    ///
    /// This is the "Parse, Don't Validate" pattern — we parse the unregistered
    /// state into a registered state once, rather than checking a flag repeatedly.
    #[allow(clippy::result_large_err)] // By design: Err returns self to continue registration
    pub fn try_register(self) -> Result<RegisteredState, Self> {
        match (&self.nick, &self.user) {
            (Some(nick), Some(user)) if !self.cap_negotiating => {
                Ok(RegisteredState {
                    session_id: Uuid::new_v4(),
                    device_id: None, // Set by SASL handler after registration
                    nick: nick.clone(),
                    user: user.clone(),
                    realname: self.realname.unwrap_or_default(),
                    capabilities: self.capabilities,
                    account: self.account,
                    is_tls: self.is_tls,
                    certfp: self.certfp,
                    cap_version: self.cap_version,
                    // Post-registration state starts fresh
                    failed_oper_attempts: 0,
                    last_oper_attempt: None,
                    active_batch: None,
                    active_batch_ref: None,
                    // Ping timeout tracking starts fresh
                    last_activity: Instant::now(),
                    ping_pending: false,
                    ping_sent_at: None,
                    // Rate limiting for KNOCK and INVITE commands
                    knock_timestamps: HashMap::new(),
                    invite_timestamps: HashMap::new(),
                    // SASL state preserved for post-registration re-authentication
                    sasl_state: self.sasl_state,
                    sasl_buffer: self.sasl_buffer,
                    // Reattach info is carried forward from UnregisteredState
                    reattach_info: self.reattach_info,
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
    /// Unique identifier for this connection (for bouncer/multiclient).
    pub session_id: SessionId,
    /// Device identifier (extracted from SASL username or ident).
    pub device_id: Option<DeviceId>,
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
    /// CAP protocol version (preserved from registration).
    pub cap_version: u32,
    /// Last time we received any message from this client (for ping timeout).
    pub last_activity: Instant,
    /// Whether we've sent a PING and are waiting for PONG.
    pub ping_pending: bool,
    /// When we sent the pending PING (for timeout calculation).
    pub ping_sent_at: Option<Instant>,
    /// Track last KNOCK time per channel (for rate limiting).
    /// Key: lowercase channel name, Value: timestamp of last knock.
    pub knock_timestamps: HashMap<String, Instant>,
    /// Track last INVITE time per target user (for rate limiting).
    /// Key: lowercase target nick, Value: timestamp of last invite.
    pub invite_timestamps: HashMap<String, Instant>,
    /// SASL authentication state (for post-registration re-authentication).
    pub sasl_state: SaslState,
    /// Buffer for accumulating chunked SASL data.
    pub sasl_buffer: String,
    /// Reattach info for bouncer session auto-replay (consumed after registration).
    pub reattach_info: Option<ReattachInfo>,
}

impl RegisteredState {
    /// Check if a capability is enabled (for tests).
    #[cfg(test)]
    #[inline]
    pub fn has_cap(&self, cap: &str) -> bool {
        self.capabilities.contains(cap)
    }
}

impl SessionState for RegisteredState {
    fn nick(&self) -> Option<&str> {
        Some(&self.nick)
    }

    fn set_nick(&mut self, nick: String) {
        self.nick = nick;
    }

    fn is_registered(&self) -> bool {
        true
    }

    fn is_server(&self) -> bool {
        false
    }

    fn session_id(&self) -> SessionId {
        self.session_id
    }

    fn set_device_id(&mut self, device_id: Option<DeviceId>) {
        self.device_id = device_id;
    }

    fn capabilities(&self) -> &HashSet<String> {
        &self.capabilities
    }

    fn capabilities_mut(&mut self) -> &mut HashSet<String> {
        &mut self.capabilities
    }

    fn set_cap_negotiating(&mut self, _negotiating: bool) {
        // No-op for registered state - CAP END was already called
    }

    fn set_cap_version(&mut self, version: u32) {
        self.cap_version = version;
    }

    fn is_tls(&self) -> bool {
        self.is_tls
    }

    fn certfp(&self) -> Option<&str> {
        self.certfp.as_deref()
    }

    fn active_batch_mut(&mut self) -> &mut Option<BatchState> {
        &mut self.active_batch
    }

    fn active_batch_ref(&self) -> Option<&str> {
        self.active_batch_ref.as_deref()
    }
}

impl SaslAccess for RegisteredState {
    fn sasl_state(&self) -> &SaslState {
        &self.sasl_state
    }

    fn set_sasl_state(&mut self, state: SaslState) {
        self.sasl_state = state;
    }

    fn sasl_buffer(&self) -> &str {
        &self.sasl_buffer
    }

    fn sasl_buffer_mut(&mut self) -> &mut String {
        &mut self.sasl_buffer
    }

    fn set_account(&mut self, account: Option<String>) {
        self.account = account;
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

        // SAFETY: Test code - expect() is acceptable for test assertions
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
            session_id: Uuid::new_v4(),
            device_id: None,
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
            cap_version: 302,
            last_activity: Instant::now(),
            ping_pending: false,
            ping_sent_at: None,
            knock_timestamps: HashMap::new(),
            invite_timestamps: HashMap::new(),
            sasl_state: SaslState::default(),
            sasl_buffer: String::new(),
            reattach_info: None,
        };

        assert!(state.has_cap("echo-message"));
        assert!(!state.has_cap("server-time"));
    }
}
