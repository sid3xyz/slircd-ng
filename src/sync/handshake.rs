//! S2S Handshake State Machine.
//!
//! Manages the transition from an unconnected socket to a fully synced server link.
//! Implements the TS6-like handshake protocol defined in `docs/S2S_PROTOCOL.md`.

use crate::config::LinkBlock;
use slirc_crdt::clock::ServerId;
use slirc_proto::Command;

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
    #[allow(dead_code)]
    ProtocolError(String),
    #[allow(dead_code)]
    UnknownServer(String),
}

pub struct HandshakeMachine {
    pub state: HandshakeState,
    pub remote_name: Option<String>,
    pub remote_pass: Option<String>,
    pub remote_sid: Option<ServerId>,

    // Local identity
    pub local_sid: ServerId,
    pub local_name: String,
    pub local_desc: String,
}

impl HandshakeMachine {
    pub fn new(local_sid: ServerId, local_name: String, local_desc: String) -> Self {
        Self {
            state: HandshakeState::Unconnected,
            remote_name: None,
            remote_pass: None,
            remote_sid: None,
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

    fn handle_outbound_step(
        &mut self,
        command: Command,
        links: &[LinkBlock],
    ) -> Result<Vec<Command>, HandshakeError> {
        match command {
            Command::PASS(pass) => {
                self.remote_pass = Some(pass);
                Ok(vec![])
            }
            Command::Raw(cmd, args) if cmd == "PASS" => {
                if let Some(pass) = args.first() {
                    self.remote_pass = Some(pass.clone());
                }
                Ok(vec![])
            }
            Command::SERVER(name, _hopcount, sid, _info) => {
                self.remote_name = Some(name.clone());
                self.remote_sid = Some(ServerId::new(&sid));

                // Verify credentials
                self.verify_credentials(links)?;

                // Transition to Bursting
                self.state = HandshakeState::Bursting;
                Ok(vec![])
            }
            _ => Err(HandshakeError::ProtocolError(format!(
                "Unexpected command in OutboundInitiated: {:?}",
                command
            ))),
        }
    }

    fn handle_inbound_step(
        &mut self,
        command: Command,
        links: &[LinkBlock],
    ) -> Result<Vec<Command>, HandshakeError> {
        match command {
            Command::PASS(pass) => {
                self.remote_pass = Some(pass);
                Ok(vec![])
            }
            Command::Raw(cmd, args) if cmd == "PASS" => {
                if let Some(pass) = args.first() {
                    self.remote_pass = Some(pass.clone());
                }
                Ok(vec![])
            }
            Command::SERVER(name, _hopcount, sid, _info) => {
                self.remote_name = Some(name.clone());
                self.remote_sid = Some(ServerId::new(&sid));

                // Verify credentials
                let link = self.verify_credentials(links)?;

                // Send our credentials
                let responses = vec![
                    // PASS <password> TS=6 :<sid>
                    Command::Raw(
                        "PASS".to_string(),
                        vec![
                            link.password.clone(),
                            "TS=6".to_string(),
                            self.local_sid.as_str().to_string(),
                        ],
                    ),
                    // SERVER <name> <hopcount> <description>
                    Command::SERVER(
                        self.local_name.clone(),
                        1,
                        self.local_sid.as_str().to_string(),
                        self.local_desc.clone(),
                    ),
                ];

                self.state = HandshakeState::Bursting;
                Ok(responses)
            }
            _ => Err(HandshakeError::ProtocolError(format!(
                "Unexpected command in InboundReceived: {:?}",
                command
            ))),
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
