//! S2S Handshake State Machine.
//!
//! Manages the transition from an unconnected socket to a fully synced server link.
//! Implements the TS6-like handshake protocol defined in `docs/S2S_PROTOCOL.md`.

use crate::config::LinkBlock;
use slirc_proto::Command;
use slirc_proto::sync::clock::ServerId;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum HandshakeState {
    /// Initial state.
    Unconnected,
    /// We initiated the connection (outbound).
    /// We have sent PASS and SERVER, waiting for remote PASS/SERVER.
    OutboundInitiated,
    /// We received a connection (inbound).
    /// We are waiting for PASS and SERVER.
    InboundReceived,
    /// Handshake complete, exchanging burst data.
    Bursting,
    /// Fully synchronized.
    Synced,
}

#[derive(Debug)]
pub enum HandshakeError {
    InvalidStateTransition,
    AuthenticationFailed,
    ProtocolError(String),
    UnknownServer(String),
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::InvalidStateTransition => {
                write!(f, "invalid handshake state transition")
            }
            HandshakeError::AuthenticationFailed => write!(f, "handshake authentication failed"),
            HandshakeError::ProtocolError(msg) => write!(f, "handshake protocol error: {msg}"),
            HandshakeError::UnknownServer(name) => write!(f, "unknown server: {name}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

pub struct HandshakeMachine {
    pub state: HandshakeState,
    pub remote_name: Option<String>,
    pub remote_pass: Option<String>,
    pub remote_sid: Option<ServerId>,
    pub remote_sid_from_pass: Option<ServerId>,
    pub remote_info: Option<String>,
    pub remote_capab: Option<Vec<String>>,
    pub remote_svinfo: Option<(u32, u32, u32, u64)>,

    // Local identity
    pub local_sid: ServerId,
    pub local_name: String,
    pub local_desc: String,
}

/// Capabilities supported by slircd-ng for S2S.
pub const SUPPORTED_CAPABS: &[&str] = &[
    "QS", "ENCAP", "EX", "IE", "UNKLN", "KLN", "GLN", "HOPS", "CHW", "EOB", "KNOCK", "TB",
    "SERVICES",
];

impl HandshakeMachine {
    pub fn new(local_sid: ServerId, local_name: String, local_desc: String) -> Self {
        Self {
            state: HandshakeState::Unconnected,
            remote_name: None,
            remote_pass: None,
            remote_sid: None,
            remote_sid_from_pass: None,
            remote_info: None,
            remote_capab: None,
            remote_svinfo: None,
            local_sid,
            local_name,
            local_desc,
        }
    }

    pub fn transition(&mut self, new_state: HandshakeState) {
        self.state = new_state;
    }

    pub fn step(
        &mut self,
        command: Command,
        links: &[LinkBlock],
    ) -> Result<Vec<Command>, HandshakeError> {
        match self.state {
            HandshakeState::Unconnected => {
                // We shouldn't be stepping in Unconnected unless we just started.
                // If we are Unconnected, we expect nothing until we transition to InboundReceived or OutboundInitiated?
                // Actually, handle_inbound_connection sets state to InboundReceived?
                // Or we treat the first message as starting InboundReceived?
                Err(HandshakeError::InvalidStateTransition)
            }
            HandshakeState::OutboundInitiated => self.handle_outbound_step(command, links),
            HandshakeState::InboundReceived => self.handle_inbound_step(command, links),
            HandshakeState::Bursting | HandshakeState::Synced => {
                // Handshake is done, these states shouldn't process handshake commands via step?
                // Or maybe they process BURST/SJOIN?
                // For now, step is for handshake only.
                Ok(vec![])
            }
        }
    }

    fn check_handshake_complete(&mut self, links: &[LinkBlock]) -> Result<bool, HandshakeError> {
        if self.remote_pass.is_some()
            && self.remote_name.is_some()
            && self.remote_sid.is_some()
            && self.remote_svinfo.is_some()
            && self.remote_capab.is_some()
        {
            self.verify_credentials(links)?;
            self.state = HandshakeState::Bursting;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn handle_outbound_step(
        &mut self,
        command: Command,
        links: &[LinkBlock],
    ) -> Result<Vec<Command>, HandshakeError> {
        match command {
            Command::PASS(_) => {
                return Err(HandshakeError::ProtocolError(
                    "S2S handshake requires TS6 PASS (PASS <password> TS 6 :<sid>)".to_string(),
                ));
            }
            Command::PassTs6 { password, sid } => {
                self.remote_pass = Some(password);
                let pass_sid = ServerId::new(&sid);
                self.remote_sid_from_pass = Some(pass_sid.clone());

                if let Some(existing) = &self.remote_sid {
                    if existing != &pass_sid {
                        return Err(HandshakeError::ProtocolError(format!(
                            "PASS/SERVER SID mismatch (PASS={}, SERVER={})",
                            pass_sid.as_str(),
                            existing.as_str(),
                        )));
                    }
                } else {
                    self.remote_sid = Some(pass_sid);
                }
            }
            Command::CAPAB(caps) => {
                self.remote_capab = Some(caps);
            }
            Command::SVINFO(v, m, z, t) => {
                self.remote_svinfo = Some((v, m, z, t));
            }
            Command::SERVER(name, _hopcount, sid, info) => {
                self.remote_name = Some(name.clone());
                let server_sid = ServerId::new(&sid);
                if let Some(pass_sid) = &self.remote_sid_from_pass
                    && pass_sid != &server_sid
                {
                    return Err(HandshakeError::ProtocolError(format!(
                        "PASS/SERVER SID mismatch (PASS={}, SERVER={})",
                        pass_sid.as_str(),
                        server_sid.as_str(),
                    )));
                }
                self.remote_sid = Some(server_sid);
                self.remote_info = Some(info);
            }
            Command::CAP(_, _, _, _) => {
                // Ignore CAP negotiation for now in S2S
            }
            _ => {
                return Err(HandshakeError::ProtocolError(format!(
                    "Unexpected command in OutboundInitiated: {:?}",
                    command
                )));
            }
        }

        self.check_handshake_complete(links)?;
        Ok(vec![])
    }

    fn handle_inbound_step(
        &mut self,
        command: Command,
        links: &[LinkBlock],
    ) -> Result<Vec<Command>, HandshakeError> {
        match command {
            Command::PASS(_) => {
                return Err(HandshakeError::ProtocolError(
                    "S2S handshake requires TS6 PASS (PASS <password> TS 6 :<sid>)".to_string(),
                ));
            }
            Command::PassTs6 { password, sid } => {
                self.remote_pass = Some(password);
                let pass_sid = ServerId::new(&sid);
                self.remote_sid_from_pass = Some(pass_sid.clone());

                if let Some(existing) = &self.remote_sid {
                    if existing != &pass_sid {
                        return Err(HandshakeError::ProtocolError(format!(
                            "PASS/SERVER SID mismatch (PASS={}, SERVER={})",
                            pass_sid.as_str(),
                            existing.as_str(),
                        )));
                    }
                } else {
                    self.remote_sid = Some(pass_sid);
                }
            }
            Command::CAPAB(caps) => {
                self.remote_capab = Some(caps);
            }
            Command::SVINFO(v, m, z, t) => {
                self.remote_svinfo = Some((v, m, z, t));
            }
            Command::SERVER(name, _hopcount, sid, info) => {
                self.remote_name = Some(name.clone());
                let server_sid = ServerId::new(&sid);
                if let Some(pass_sid) = &self.remote_sid_from_pass
                    && pass_sid != &server_sid
                {
                    return Err(HandshakeError::ProtocolError(format!(
                        "PASS/SERVER SID mismatch (PASS={}, SERVER={})",
                        pass_sid.as_str(),
                        server_sid.as_str(),
                    )));
                }
                self.remote_sid = Some(server_sid);
                self.remote_info = Some(info);
            }
            Command::CAP(_, _, _, _) => {
                // Ignore CAP negotiation for now in S2S
            }
            _ => {
                return Err(HandshakeError::ProtocolError(format!(
                    "Unexpected command in InboundReceived: {:?}",
                    command
                )));
            }
        }

        if self.check_handshake_complete(links)? {
            let link = self.verify_credentials(links)?;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| {
                    HandshakeError::ProtocolError(format!("system clock before UNIX_EPOCH: {e}"))
                })?
                .as_secs();

            // Send our credentials
            let responses = vec![
                Command::PassTs6 {
                    password: link.password.clone(),
                    sid: self.local_sid.as_str().to_string(),
                },
                Command::CAPAB(SUPPORTED_CAPABS.iter().map(|s| s.to_string()).collect()),
                Command::SERVER(
                    self.local_name.clone(),
                    1,
                    self.local_sid.as_str().to_string(),
                    self.local_desc.clone(),
                ),
                Command::SVINFO(6, 6, 0, now_secs),
            ];
            Ok(responses)
        } else {
            Ok(vec![])
        }
    }

    fn verify_credentials<'a>(
        &self,
        links: &'a [LinkBlock],
    ) -> Result<&'a LinkBlock, HandshakeError> {
        let name = self
            .remote_name
            .as_ref()
            .ok_or(HandshakeError::ProtocolError(
                "Missing SERVER name".to_string(),
            ))?;
        let pass = self
            .remote_pass
            .as_ref()
            .ok_or(HandshakeError::AuthenticationFailed)?;

        let link = links
            .iter()
            .find(|l| &l.name == name)
            .ok_or_else(|| HandshakeError::UnknownServer(name.clone()))?;

        if &link.password != pass {
            return Err(HandshakeError::AuthenticationFailed);
        }

        Ok(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_machine() -> HandshakeMachine {
        HandshakeMachine::new(
            ServerId::new("001"),
            "test.server.com".to_string(),
            "Test Server".to_string(),
        )
    }

    // ========================================================================
    // HandshakeMachine::new tests
    // ========================================================================

    #[test]
    fn handshake_machine_starts_unconnected() {
        let machine = make_machine();
        assert_eq!(machine.state, HandshakeState::Unconnected);
    }

    #[test]
    fn handshake_machine_stores_local_info() {
        let machine = make_machine();
        assert_eq!(machine.local_sid.as_str(), "001");
        assert_eq!(machine.local_name, "test.server.com");
        assert_eq!(machine.local_desc, "Test Server");
    }

    #[test]
    fn handshake_machine_remote_fields_initially_none() {
        let machine = make_machine();
        assert!(machine.remote_name.is_none());
        assert!(machine.remote_pass.is_none());
        assert!(machine.remote_sid.is_none());
        assert!(machine.remote_info.is_none());
        assert!(machine.remote_capab.is_none());
        assert!(machine.remote_svinfo.is_none());
    }

    // ========================================================================
    // HandshakeMachine::transition tests
    // ========================================================================

    #[test]
    fn transition_to_outbound_initiated() {
        let mut machine = make_machine();
        machine.transition(HandshakeState::OutboundInitiated);
        assert_eq!(machine.state, HandshakeState::OutboundInitiated);
    }

    #[test]
    fn transition_to_bursting() {
        let mut machine = make_machine();
        machine.transition(HandshakeState::Bursting);
        assert_eq!(machine.state, HandshakeState::Bursting);
    }

    #[test]
    fn transition_to_synced() {
        let mut machine = make_machine();
        machine.transition(HandshakeState::Synced);
        assert_eq!(machine.state, HandshakeState::Synced);
    }

    // ========================================================================
    // HandshakeState equality tests
    // ========================================================================

    #[test]
    fn handshake_states_are_equal() {
        assert_eq!(HandshakeState::Unconnected, HandshakeState::Unconnected);
        assert_eq!(HandshakeState::Synced, HandshakeState::Synced);
    }

    #[test]
    fn handshake_states_are_not_equal() {
        assert_ne!(HandshakeState::Unconnected, HandshakeState::Synced);
        assert_ne!(HandshakeState::Bursting, HandshakeState::OutboundInitiated);
    }

    // ========================================================================
    // step from Unconnected tests
    // ========================================================================

    #[test]
    fn step_from_unconnected_returns_error() {
        let mut machine = make_machine();
        let result = machine.step(Command::PING("test".to_string(), None), &[]);
        assert!(matches!(
            result,
            Err(HandshakeError::InvalidStateTransition)
        ));
    }

    // ========================================================================
    // step from Synced tests
    // ========================================================================

    #[test]
    fn step_from_synced_returns_empty() {
        let mut machine = make_machine();
        machine.transition(HandshakeState::Synced);
        let result = machine.step(Command::PING("test".to_string(), None), &[]);
        assert!(matches!(result, Ok(commands) if commands.is_empty()));
    }
}
