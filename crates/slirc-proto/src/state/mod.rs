//! Sans-IO connection state machine for IRC protocol handling.
//!
//! This module provides a "sans-IO" state machine for managing IRC connection
//! lifecycle. It does not perform actual I/O—instead, it consumes events
//! (parsed messages) and produces actions (messages to send).
//!
//! # Design Philosophy
//!
//! The state machine is designed to be:
//! - **Sans-IO**: No network calls, timers, or blocking. Pure state transitions.
//! - **Runtime-agnostic**: Works with tokio, async-std, or blocking code.
//! - **Testable**: Easy to unit test without mocking network.
//!
//! # Example
//!
//! ```
//! use slirc_proto::state::{HandshakeMachine, HandshakeConfig, HandshakeAction};
//! use slirc_proto::MessageRef;
//!
//! let config = HandshakeConfig {
//!     nickname: "testbot".to_string(),
//!     username: "bot".to_string(),
//!     realname: "Test Bot".to_string(),
//!     password: None,
//!     request_caps: vec!["multi-prefix".to_string(), "sasl".to_string()],
//!     sasl_credentials: None,
//! };
//!
//! let mut machine = HandshakeMachine::new(config);
//!
//! // Get initial actions (CAP LS, NICK, USER)
//! let actions = machine.start();
//! for action in actions {
//!     // Send action.message() to server
//! }
//!
//! // Feed server responses
//! let cap_ack = MessageRef::parse(":server CAP * ACK :multi-prefix sasl").unwrap();
//! let actions = machine.feed(&cap_ack);
//! // Process actions...
//! ```

mod sync;
mod tracker;

pub use tracker::HandshakeMachine;

use crate::Message;

/// Current state of the IRC connection handshake.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConnectionState {
    /// Initial state, not yet connected.
    #[default]
    Disconnected,
    /// Sent CAP LS, awaiting capability list.
    CapabilityNegotiation,
    /// Performing SASL authentication.
    Authenticating,
    /// Sent CAP END, awaiting welcome (001).
    Registering,
    /// Received 001, fully connected.
    Connected,
    /// Connection terminated (QUIT sent or ERROR received).
    Terminated,
}

/// Configuration for the handshake state machine.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HandshakeConfig {
    /// Desired nickname.
    pub nickname: String,
    /// Username (ident).
    pub username: String,
    /// Real name / GECOS.
    pub realname: String,
    /// Server password, if required.
    pub password: Option<String>,
    /// Capabilities to request (e.g., "multi-prefix", "sasl").
    pub request_caps: Vec<String>,
    /// SASL credentials, if SASL authentication is desired.
    pub sasl_credentials: Option<SaslCredentials>,
}

/// SASL authentication credentials.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SaslCredentials {
    /// Account name (often same as nickname).
    pub account: String,
    /// Password.
    pub password: String,
}

/// Actions produced by the handshake state machine.
///
/// The caller is responsible for sending these messages to the server.
#[derive(Clone, Debug)]
pub enum HandshakeAction {
    /// Send this message to the server.
    ///
    /// Boxed to reduce enum size variance (Message is large).
    Send(Box<Message>),
    /// Connection is complete, proceed to normal operation.
    Complete,
    /// An error occurred during handshake.
    Error(HandshakeError),
}

/// Errors that can occur during handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    /// Server rejected capability request.
    CapabilityRejected(Vec<String>),
    /// SASL authentication failed.
    SaslFailed(String),
    /// Nickname collision.
    NicknameInUse(String),
    /// Server sent ERROR.
    ServerError(String),
    /// Unexpected message during handshake.
    ProtocolError(String),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapabilityRejected(caps) => {
                write!(f, "capability rejected: {}", caps.join(", "))
            }
            Self::SaslFailed(reason) => write!(f, "SASL authentication failed: {}", reason),
            Self::NicknameInUse(nick) => write!(f, "nickname in use: {}", nick),
            Self::ServerError(msg) => write!(f, "server error: {}", msg),
            Self::ProtocolError(msg) => write!(f, "protocol error: {}", msg),
        }
    }
}

impl std::error::Error for HandshakeError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ConnectionState tests
    #[test]
    fn test_connection_state_default() {
        let state = ConnectionState::default();
        assert_eq!(state, ConnectionState::Disconnected);
    }

    #[test]
    fn test_connection_state_clone_eq() {
        let state1 = ConnectionState::Connected;
        let state2 = state1.clone();
        assert_eq!(state1, state2);
    }

    #[test]
    fn test_connection_state_variants() {
        // Ensure all variants are distinct
        let states = [
            ConnectionState::Disconnected,
            ConnectionState::CapabilityNegotiation,
            ConnectionState::Authenticating,
            ConnectionState::Registering,
            ConnectionState::Connected,
            ConnectionState::Terminated,
        ];
        for i in 0..states.len() {
            for j in 0..states.len() {
                if i == j {
                    assert_eq!(states[i], states[j]);
                } else {
                    assert_ne!(states[i], states[j]);
                }
            }
        }
    }

    // HandshakeConfig tests
    #[test]
    fn test_handshake_config_new() {
        let config = HandshakeConfig {
            nickname: "testnick".to_string(),
            username: "testuser".to_string(),
            realname: "Test User".to_string(),
            password: None,
            request_caps: vec!["multi-prefix".to_string()],
            sasl_credentials: None,
        };
        assert_eq!(config.nickname, "testnick");
        assert_eq!(config.username, "testuser");
        assert_eq!(config.realname, "Test User");
        assert!(config.password.is_none());
        assert_eq!(config.request_caps.len(), 1);
        assert!(config.sasl_credentials.is_none());
    }

    #[test]
    fn test_handshake_config_with_password() {
        let config = HandshakeConfig {
            nickname: "nick".to_string(),
            username: "user".to_string(),
            realname: "Real".to_string(),
            password: Some("secret".to_string()),
            request_caps: vec![],
            sasl_credentials: None,
        };
        assert_eq!(config.password, Some("secret".to_string()));
    }

    #[test]
    fn test_handshake_config_with_sasl() {
        let creds = SaslCredentials {
            account: "myaccount".to_string(),
            password: "mypassword".to_string(),
        };
        let config = HandshakeConfig {
            nickname: "nick".to_string(),
            username: "user".to_string(),
            realname: "Real".to_string(),
            password: None,
            request_caps: vec!["sasl".to_string()],
            sasl_credentials: Some(creds),
        };
        assert!(config.sasl_credentials.is_some());
        let creds = config.sasl_credentials.unwrap();
        assert_eq!(creds.account, "myaccount");
        assert_eq!(creds.password, "mypassword");
    }

    // SaslCredentials tests
    #[test]
    fn test_sasl_credentials() {
        let creds = SaslCredentials {
            account: "account".to_string(),
            password: "password".to_string(),
        };
        assert_eq!(creds.account, "account");
        assert_eq!(creds.password, "password");
    }

    // HandshakeError tests
    #[test]
    fn test_handshake_error_display_capability_rejected() {
        let err = HandshakeError::CapabilityRejected(vec!["cap1".to_string(), "cap2".to_string()]);
        assert_eq!(err.to_string(), "capability rejected: cap1, cap2");
    }

    #[test]
    fn test_handshake_error_display_sasl_failed() {
        let err = HandshakeError::SaslFailed("invalid credentials".to_string());
        assert_eq!(
            err.to_string(),
            "SASL authentication failed: invalid credentials"
        );
    }

    #[test]
    fn test_handshake_error_display_nickname_in_use() {
        let err = HandshakeError::NicknameInUse("taken".to_string());
        assert_eq!(err.to_string(), "nickname in use: taken");
    }

    #[test]
    fn test_handshake_error_display_server_error() {
        let err = HandshakeError::ServerError("connection refused".to_string());
        assert_eq!(err.to_string(), "server error: connection refused");
    }

    #[test]
    fn test_handshake_error_display_protocol_error() {
        let err = HandshakeError::ProtocolError("unexpected message".to_string());
        assert_eq!(err.to_string(), "protocol error: unexpected message");
    }

    #[test]
    fn test_handshake_error_eq() {
        let err1 = HandshakeError::NicknameInUse("nick".to_string());
        let err2 = HandshakeError::NicknameInUse("nick".to_string());
        let err3 = HandshakeError::NicknameInUse("other".to_string());
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);
    }

    // HandshakeAction tests
    #[test]
    fn test_handshake_action_complete() {
        let action = HandshakeAction::Complete;
        assert!(matches!(action, HandshakeAction::Complete));
    }

    #[test]
    fn test_handshake_action_error() {
        let action = HandshakeAction::Error(HandshakeError::ServerError("test".to_string()));
        assert!(matches!(action, HandshakeAction::Error(_)));
    }

    #[test]
    fn test_handshake_action_send() {
        let msg = Box::new(Message::from(crate::command::Command::NICK(
            "test".to_string(),
        )));
        let action = HandshakeAction::Send(msg);
        assert!(matches!(action, HandshakeAction::Send(_)));
    }
}
