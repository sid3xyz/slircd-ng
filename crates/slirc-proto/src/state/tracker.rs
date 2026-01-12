//! Handshake state machine core implementation.

use std::collections::HashSet;

use crate::message::MessageRef;

use super::{ConnectionState, HandshakeAction, HandshakeConfig};

/// Sans-IO state machine for IRC connection handshake.
///
/// This handles the CAP -> AUTHENTICATE -> NICK/USER -> 001 flow.
#[derive(Clone, Debug)]
pub struct HandshakeMachine {
    pub(super) config: HandshakeConfig,
    pub(super) state: ConnectionState,
    /// Capabilities acknowledged by server.
    pub(super) enabled_caps: HashSet<String>,
    /// Capabilities available on server.
    pub(super) available_caps: HashSet<String>,
    /// Whether we've sent NICK/USER.
    pub(super) registration_sent: bool,
    /// Whether we're waiting for more CAP LS (multiline).
    pub(super) waiting_for_more_caps: bool,
}

impl HandshakeMachine {
    /// Create a new handshake state machine with the given configuration.
    #[must_use]
    pub fn new(config: HandshakeConfig) -> Self {
        Self {
            config,
            state: ConnectionState::Disconnected,
            enabled_caps: HashSet::new(),
            available_caps: HashSet::new(),
            registration_sent: false,
            waiting_for_more_caps: false,
        }
    }

    /// Get the current connection state.
    #[must_use]
    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    /// Get the set of enabled capabilities.
    #[must_use]
    pub fn enabled_caps(&self) -> &HashSet<String> {
        &self.enabled_caps
    }

    /// Get the set of available capabilities.
    #[must_use]
    pub fn available_caps(&self) -> &HashSet<String> {
        &self.available_caps
    }

    /// Start the handshake. Returns initial messages to send.
    #[must_use]
    pub fn start(&mut self) -> Vec<HandshakeAction> {
        self.state = ConnectionState::CapabilityNegotiation;
        let mut actions = Vec::new();

        // Send PASS if configured
        if let Some(ref pass) = self.config.password {
            actions.push(HandshakeAction::Send(Box::new(
                crate::command::Command::PASS(pass.clone()).into(),
            )));
        }

        // Request capability list (302 = IRCv3.2)
        actions.push(HandshakeAction::Send(Box::new(
            crate::command::Command::CAP(
                None,
                crate::command::CapSubCommand::LS,
                Some("302".to_string()),
                None,
            )
            .into(),
        )));

        actions
    }

    /// Feed a parsed message to the state machine.
    ///
    /// Returns actions to perform (messages to send, completion, or errors).
    #[must_use]
    pub fn feed(&mut self, msg: &MessageRef<'_>) -> Vec<HandshakeAction> {
        match self.state {
            ConnectionState::Disconnected => vec![],
            ConnectionState::CapabilityNegotiation => self.handle_cap_negotiation(msg),
            ConnectionState::Authenticating => self.handle_authentication(msg),
            ConnectionState::Registering => self.handle_registration(msg),
            ConnectionState::Connected | ConnectionState::Terminated => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::HandshakeConfig;

    fn make_config() -> HandshakeConfig {
        HandshakeConfig {
            nickname: "testbot".to_string(),
            username: "bot".to_string(),
            realname: "Test Bot".to_string(),
            password: None,
            request_caps: vec!["multi-prefix".to_string()],
            sasl_credentials: None,
        }
    }

    #[test]
    fn test_start_sends_cap_ls() {
        let mut machine = HandshakeMachine::new(make_config());
        let actions = machine.start();

        assert_eq!(machine.state(), &ConnectionState::CapabilityNegotiation);
        assert_eq!(actions.len(), 1);

        if let HandshakeAction::Send(msg) = &actions[0] {
            assert!(matches!(
                msg.command,
                crate::command::Command::CAP(_, _, _, _)
            ));
        } else {
            panic!("Expected Send action");
        }
    }

    #[test]
    fn test_cap_ls_then_req() {
        let mut machine = HandshakeMachine::new(make_config());
        let _ = machine.start();

        let cap_ls = MessageRef::parse(":server CAP * LS :multi-prefix sasl").unwrap();
        let actions = machine.feed(&cap_ls);

        assert!(machine.available_caps().contains("multi-prefix"));
        assert!(machine.available_caps().contains("sasl"));

        // Should request multi-prefix (since it's in request_caps)
        assert!(!actions.is_empty());
        if let HandshakeAction::Send(msg) = &actions[0] {
            assert!(matches!(
                msg.command,
                crate::command::Command::CAP(_, crate::command::CapSubCommand::REQ, _, _)
            ));
        }
    }

    #[test]
    fn test_cap_ack_then_end() {
        let mut machine = HandshakeMachine::new(make_config());
        let _ = machine.start();

        let cap_ls = MessageRef::parse(":server CAP * LS :multi-prefix").unwrap();
        let _ = machine.feed(&cap_ls);

        let cap_ack = MessageRef::parse(":server CAP * ACK :multi-prefix").unwrap();
        let actions = machine.feed(&cap_ack);

        assert!(machine.enabled_caps().contains("multi-prefix"));
        assert_eq!(machine.state(), &ConnectionState::Registering);

        // Should have CAP END, NICK, USER
        assert!(actions.len() >= 3);
    }

    #[test]
    fn test_welcome_completes() {
        let mut machine = HandshakeMachine::new(make_config());
        let _ = machine.start();

        // Simulate full handshake
        let cap_ls = MessageRef::parse(":server CAP * LS :").unwrap();
        let _ = machine.feed(&cap_ls);

        let welcome = MessageRef::parse(":server 001 testbot :Welcome").unwrap();
        let actions = machine.feed(&welcome);

        assert_eq!(machine.state(), &ConnectionState::Connected);
        assert!(actions
            .iter()
            .any(|a| matches!(a, HandshakeAction::Complete)));
    }
}
