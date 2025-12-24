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
                    Capability::Multiline => Some(format!(
                        "draft/multiline=max-bytes={},max-lines={}",
                        MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES
                    )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccountRegistrationConfig;

    // ─────────────────────────────────────────────────────────────────────────
    // build_cap_list_tokens tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_cap_list_tls_with_cert() {
        // TLS + cert → includes "sasl=PLAIN,EXTERNAL"
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, true, &cfg);

        assert!(
            caps.iter().any(|c| c == "sasl=PLAIN,EXTERNAL"),
            "TLS with cert should advertise SASL EXTERNAL: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_tls_no_cert() {
        // TLS, no cert → includes "sasl=PLAIN" (not EXTERNAL)
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, false, &cfg);

        assert!(
            caps.iter().any(|c| c == "sasl=PLAIN"),
            "TLS without cert should advertise SASL PLAIN only: {:?}",
            caps
        );
        assert!(
            !caps.iter().any(|c| c.contains("EXTERNAL")),
            "TLS without cert should NOT advertise EXTERNAL: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_no_tls() {
        // No TLS → SASL not advertised, TLS (STARTTLS) cap IS present
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, false, false, &cfg);

        assert!(
            !caps.iter().any(|c| c.starts_with("sasl")),
            "Non-TLS should NOT advertise SASL: {:?}",
            caps
        );
        assert!(
            caps.iter().any(|c| c == "tls"),
            "Non-TLS should advertise STARTTLS: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_tls_no_starttls() {
        // TLS connection should NOT advertise STARTTLS (already encrypted)
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, false, &cfg);

        assert!(
            !caps.iter().any(|c| c == "tls"),
            "TLS connection should NOT advertise STARTTLS: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_version_301_no_values() {
        // CAP version 301 → no values (just capability names)
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(301, true, true, &cfg);

        // SASL should be present as just "sasl", not "sasl=..."
        assert!(
            caps.iter().any(|c| c == "sasl"),
            "CAP 301 should advertise bare 'sasl': {:?}",
            caps
        );
        assert!(
            !caps.iter().any(|c| c.contains('=')),
            "CAP 301 should not have capability values (=): {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_version_302_has_values() {
        // CAP version 302 → includes values for multiline, sasl, etc.
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, true, &cfg);

        // Should have values
        let multiline_cap = caps.iter().find(|c| c.starts_with("draft/multiline="));
        assert!(
            multiline_cap.is_some(),
            "CAP 302 should have multiline with values: {:?}",
            caps
        );
        assert!(
            multiline_cap.unwrap().contains("max-bytes="),
            "Multiline should have max-bytes: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_multiline_format() {
        // Verify multiline format string includes expected fields
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, false, &cfg);

        let multiline = caps
            .iter()
            .find(|c| c.starts_with("draft/multiline="))
            .expect("Should have multiline cap");

        assert!(
            multiline.contains(&format!("max-bytes={}", MULTILINE_MAX_BYTES)),
            "Multiline should have correct max-bytes: {}",
            multiline
        );
        assert!(
            multiline.contains(&format!("max-lines={}", MULTILINE_MAX_LINES)),
            "Multiline should have correct max-lines: {}",
            multiline
        );
    }

    #[test]
    fn test_cap_list_account_registration_flags() {
        // Default config: before-connect=true, email-required=false, custom-account-name=true
        let cfg = AccountRegistrationConfig::default();
        let caps = build_cap_list_tokens(302, true, false, &cfg);

        let acct_reg = caps
            .iter()
            .find(|c| c.starts_with("draft/account-registration"))
            .expect("Should have account-registration cap");

        assert!(
            acct_reg.contains("before-connect"),
            "Should have before-connect flag: {}",
            acct_reg
        );
        assert!(
            acct_reg.contains("custom-account-name"),
            "Should have custom-account-name flag: {}",
            acct_reg
        );
        assert!(
            !acct_reg.contains("email-required"),
            "Should NOT have email-required by default: {}",
            acct_reg
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // pack_cap_ls_lines tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_pack_cap_ls_single_line() {
        // Few caps → single line
        let caps = vec![
            "multi-prefix".to_string(),
            "server-time".to_string(),
            "sasl".to_string(),
        ];
        let lines = pack_cap_ls_lines("irc.example.net", "nick", &caps);

        assert_eq!(lines.len(), 1, "Should fit in one line");
        assert!(
            lines[0].contains("multi-prefix"),
            "Line should contain caps"
        );
    }

    #[test]
    fn test_pack_cap_ls_empty() {
        // No caps → single empty line
        let caps: Vec<String> = vec![];
        let lines = pack_cap_ls_lines("irc.example.net", "nick", &caps);

        assert_eq!(lines.len(), 1, "Should have one line");
        assert!(lines[0].is_empty(), "Line should be empty");
    }

    #[test]
    fn test_pack_cap_ls_multi_line() {
        // Many long caps → multiple lines
        let caps: Vec<String> = (0..30)
            .map(|i| format!("very-long-capability-name-{:02}", i))
            .collect();
        let lines = pack_cap_ls_lines("irc.example.net", "nick", &caps);

        assert!(
            lines.len() > 1,
            "Should need multiple lines for 30 long caps, got {}",
            lines.len()
        );

        // Verify all caps are included across lines
        let combined: String = lines.join(" ");
        for cap in &caps {
            assert!(
                combined.contains(cap),
                "Missing cap {} in output",
                cap
            );
        }
    }

    #[test]
    fn test_pack_cap_ls_long_cap_still_works() {
        // Single very long cap → still included (edge case)
        let long_cap = "x".repeat(400); // Very long but should still fit
        let caps = vec![long_cap.clone()];
        let lines = pack_cap_ls_lines("irc.example.net", "nick", &caps);

        assert!(!lines.is_empty(), "Should produce at least one line");
        // The long cap should be in the output somewhere
        let combined: String = lines.join(" ");
        assert!(
            combined.contains(&long_cap),
            "Very long cap should be included"
        );
    }
}
