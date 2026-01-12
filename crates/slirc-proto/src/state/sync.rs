//! State transition handlers for handshake phases.

use crate::command::Command;
use crate::message::MessageRef;

use super::tracker::HandshakeMachine;
use super::{ConnectionState, HandshakeAction, HandshakeError};

impl HandshakeMachine {
    pub(super) fn handle_cap_negotiation(&mut self, msg: &MessageRef<'_>) -> Vec<HandshakeAction> {
        let mut actions = Vec::new();

        if msg.command.name.eq_ignore_ascii_case("CAP") {
            let subcmd = msg.arg(1).unwrap_or("");
            match subcmd.to_ascii_uppercase().as_str() {
                "LS" => {
                    // Check for multiline (* prefix)
                    let (is_multiline, caps_str) = if msg.arg(2) == Some("*") {
                        (true, msg.arg(3).unwrap_or(""))
                    } else {
                        (false, msg.arg(2).unwrap_or(""))
                    };

                    // Parse available capabilities
                    for cap in caps_str.split_whitespace() {
                        // Handle capability values (cap=value)
                        let cap_name = cap.split('=').next().unwrap_or(cap);
                        self.available_caps.insert(cap_name.to_string());
                    }

                    if is_multiline {
                        self.waiting_for_more_caps = true;
                        return actions;
                    }

                    self.waiting_for_more_caps = false;

                    // Request capabilities we want that are available
                    let to_request: Vec<_> = self
                        .config
                        .request_caps
                        .iter()
                        .filter(|c| self.available_caps.contains(*c))
                        .cloned()
                        .collect();

                    if !to_request.is_empty() {
                        let caps_str = to_request.join(" ");
                        actions.push(HandshakeAction::Send(Box::new(
                            Command::CAP(
                                None,
                                crate::command::CapSubCommand::REQ,
                                None,
                                Some(caps_str),
                            )
                            .into(),
                        )));
                    } else {
                        // No caps to request, proceed to registration
                        actions.extend(self.finish_cap_negotiation());
                    }
                }
                "ACK" => {
                    let caps_str = msg.arg(2).unwrap_or("");
                    for cap in caps_str.split_whitespace() {
                        // Handle capability modifiers (-, ~, =)
                        let cap_name = cap.trim_start_matches(['-', '~', '=']);
                        if !cap.starts_with('-') {
                            self.enabled_caps.insert(cap_name.to_string());
                        }
                    }

                    // Check if SASL is enabled and we have credentials
                    if self.enabled_caps.contains("sasl") && self.config.sasl_credentials.is_some()
                    {
                        self.state = ConnectionState::Authenticating;
                        actions.push(HandshakeAction::Send(Box::new(
                            Command::AUTHENTICATE("PLAIN".to_string()).into(),
                        )));
                    } else {
                        actions.extend(self.finish_cap_negotiation());
                    }
                }
                "NAK" => {
                    let caps_str = msg.arg(2).unwrap_or("");
                    let rejected: Vec<_> = caps_str.split_whitespace().map(String::from).collect();
                    // NAK is not fatal, proceed with registration
                    actions.extend(self.finish_cap_negotiation());
                    if !rejected.is_empty() {
                        // Log but don't fail
                    }
                }
                _ => {}
            }
        }

        actions
    }

    pub(super) fn handle_authentication(&mut self, msg: &MessageRef<'_>) -> Vec<HandshakeAction> {
        let mut actions = Vec::new();

        match msg.command.name.to_ascii_uppercase().as_str() {
            "AUTHENTICATE" => {
                let param = msg.arg(0).unwrap_or("");
                if param == "+" {
                    // Server ready for SASL payload
                    if let Some(ref creds) = self.config.sasl_credentials {
                        let payload = crate::sasl::encode_plain(&creds.account, &creds.password);
                        actions.push(HandshakeAction::Send(Box::new(
                            Command::AUTHENTICATE(payload).into(),
                        )));
                    }
                }
            }
            _ => {
                let cmd = msg.command.name;
                // Numeric responses
                if let Ok(numeric) = cmd.parse::<u16>() {
                    match numeric {
                        900 => {
                            // RPL_LOGGEDIN - SASL successful
                        }
                        903 => {
                            // RPL_SASLSUCCESS
                            actions.extend(self.finish_cap_negotiation());
                        }
                        902 | 904 | 905 | 906 | 907 => {
                            // SASL failures
                            let reason = msg.arg(1).unwrap_or("unknown error").to_string();
                            actions
                                .push(HandshakeAction::Error(HandshakeError::SaslFailed(reason)));
                            // Still try to continue without SASL
                            actions.extend(self.finish_cap_negotiation());
                        }
                        _ => {}
                    }
                }
            }
        }

        actions
    }

    pub(super) fn handle_registration(&mut self, msg: &MessageRef<'_>) -> Vec<HandshakeAction> {
        let mut actions = Vec::new();

        match msg.command.name.to_ascii_uppercase().as_str() {
            "001" => {
                // RPL_WELCOME - fully connected
                self.state = ConnectionState::Connected;
                actions.push(HandshakeAction::Complete);
            }
            "433" | "432" => {
                // ERR_NICKNAMEINUSE or ERR_ERRONEUSNICKNAME
                let nick = msg.arg(1).unwrap_or(&self.config.nickname).to_string();
                actions.push(HandshakeAction::Error(HandshakeError::NicknameInUse(nick)));
            }
            "ERROR" => {
                let reason = msg.arg(0).unwrap_or("connection closed").to_string();
                self.state = ConnectionState::Terminated;
                actions.push(HandshakeAction::Error(HandshakeError::ServerError(reason)));
            }
            _ => {}
        }

        actions
    }

    pub(super) fn finish_cap_negotiation(&mut self) -> Vec<HandshakeAction> {
        self.state = ConnectionState::Registering;
        let mut actions = Vec::new();

        // Send CAP END
        actions.push(HandshakeAction::Send(Box::new(
            Command::CAP(None, crate::command::CapSubCommand::END, None, None).into(),
        )));

        // Send NICK and USER if not already sent
        if !self.registration_sent {
            self.registration_sent = true;
            actions.push(HandshakeAction::Send(Box::new(
                Command::NICK(self.config.nickname.clone()).into(),
            )));
            actions.push(HandshakeAction::Send(Box::new(
                Command::USER(
                    self.config.username.clone(),
                    "0".to_string(),
                    self.config.realname.clone(),
                )
                .into(),
            )));
        }

        actions
    }
}
