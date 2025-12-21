use super::types::{MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES, SUPPORTED_CAPS};
use crate::config::AccountRegistrationConfig;
use slirc_proto::{CapSubCommand, Capability, Command, Message, Prefix};

/// Build capability list string for CAP LS response.
///
/// `is_tls` indicates whether the connection is over TLS.
/// `has_cert` indicates whether the client presented a TLS certificate,
/// which enables SASL EXTERNAL.
///
/// SECURITY: SASL PLAIN is ONLY advertised over TLS connections to prevent
/// plaintext credential exposure. Non-TLS connections cannot use password auth.
pub fn build_cap_list_tokens(
    version: u32,
    is_tls: bool,
    has_cert: bool,
    acct_cfg: &AccountRegistrationConfig,
) -> Vec<String> {
    SUPPORTED_CAPS
        .iter()
        .filter_map(|cap| {
            // For CAP 302+, add values for caps that have them
            if version >= 302 {
                match cap {
                    Capability::Sasl => {
                        // SECURITY: Only advertise SASL over TLS
                        if !is_tls {
                            return None; // Don't advertise SASL at all on plaintext
                        }
                        if has_cert {
                            Some("sasl=PLAIN,EXTERNAL".to_string())
                        } else {
                            Some("sasl=PLAIN".to_string())
                        }
                    }
                    Capability::Multiline => {
                        Some(format!(
                            "draft/multiline=max-bytes={},max-lines={}",
                            MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES
                        ))
                    }
                    Capability::AccountRegistration => {
                        // Build flags based on server configuration
                        let mut flags = Vec::with_capacity(3);
                        if acct_cfg.custom_account_name {
                            flags.push("custom-account-name");
                        }
                        if acct_cfg.before_connect {
                            flags.push("before-connect");
                        }
                        if acct_cfg.email_required {
                            flags.push("email-required");
                        }
                        if flags.is_empty() {
                            Some("draft/account-registration".to_string())
                        } else {
                            Some(format!("draft/account-registration={}", flags.join(",")))
                        }
                    }
                    Capability::Tls => {
                        // Only advertise STARTTLS on plaintext connections
                        if is_tls {
                            None
                        } else {
                            Some("tls".to_string())
                        }
                    }
                    _ => Some(cap.as_ref().to_string()),
                }
            } else {
                // For older CAP versions, filter SASL on non-TLS
                if *cap == Capability::Sasl && !is_tls {
                    None
                } else if *cap == Capability::Tls && is_tls {
                    // Don't advertise tls (STARTTLS) on already-TLS connections
                    None
                } else {
                    Some(cap.as_ref().to_string())
                }
            }
        })
        .collect()
}

pub fn pack_cap_ls_lines(server_name: &str, nick: &str, caps: &[String]) -> Vec<String> {
    // If there are no capabilities, send a single empty line.
    if caps.is_empty() {
        return vec![String::new()];
    }

    // Helper: check whether a CAP LS line fits the IRC 512-byte limit when serialized.
    // For packing we always assume the "*" continuation marker is present, which makes
    // the check stricter than the final line requires.
    fn fits(server_name: &str, nick: &str, caps_str: &str) -> bool {
        let msg = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::LS,
                Some("*".to_string()),
                Some(caps_str.to_string()),
            ),
        };
        msg.to_string().len() <= 512
    }

    let mut lines: Vec<String> = Vec::with_capacity(4);
    let mut current = String::new();

    for cap in caps {
        let candidate = if current.is_empty() {
            cap.clone()
        } else {
            format!("{} {}", current, cap)
        };

        if fits(server_name, nick, &candidate) {
            current = candidate;
            continue;
        }

        if !current.is_empty() {
            lines.push(current);
            current = String::new();
        }

        // If a single token doesn't fit, we still have to send *something*.
        // This should be practically unreachable with sane capability values.
        if fits(server_name, nick, cap) {
            current = cap.clone();
        } else {
            lines.push(cap.clone());
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}
