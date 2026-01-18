use super::types::{MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES, SUPPORTED_CAPS};
use crate::config::{AccountRegistrationConfig, SecurityConfig, StsConfig};
use slirc_proto::{CapSubCommand, Capability, Command, Message, Prefix};

/// Parameters for building the CAP list.
pub struct CapListParams<'a> {
    /// CAP negotiation version (301, 302, etc.)
    pub version: u32,
    /// Whether the connection is over TLS
    pub is_tls: bool,
    /// Whether the client presented a TLS certificate
    pub has_cert: bool,
    /// Account registration config
    pub acct_cfg: &'a AccountRegistrationConfig,
    /// Security config
    pub sec_cfg: &'a SecurityConfig,
    /// STS (Strict Transport Security) config, if enabled
    pub sts_cfg: Option<&'a StsConfig>,
}

/// Build capability list string for CAP LS response.
///
/// SECURITY: SASL PLAIN is ONLY advertised over TLS connections to prevent
/// plaintext credential exposure. Non-TLS connections cannot use password auth.
pub fn build_cap_list_tokens(params: &CapListParams<'_>) -> Vec<String> {
    let CapListParams {
        version,
        is_tls,
        has_cert,
        acct_cfg,
        sec_cfg,
        sts_cfg,
    } = params;

    SUPPORTED_CAPS
        .iter()
        .filter_map(|cap| {
            // For CAP 302+, add values for caps that have them
            if *version >= 302 {
                match cap {
                    Capability::Sasl => {
                        // Advertise SASL on TLS connections, or if plaintext SASL is allowed.
                        if *is_tls {
                            if *has_cert {
                                Some("sasl=SCRAM-SHA-256,PLAIN,EXTERNAL".to_string())
                            } else {
                                Some("sasl=SCRAM-SHA-256,PLAIN".to_string())
                            }
                        } else if sec_cfg.allow_plaintext_sasl_plain {
                            // Insecure plaintext connection, but config allows it.
                            // Do not advertise EXTERNAL, as it requires TLS.
                            Some("sasl=SCRAM-SHA-256,PLAIN".to_string())
                        } else {
                            None
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
                        if *is_tls {
                            None
                        } else {
                            Some("tls".to_string())
                        }
                    }
                    Capability::Sts => {
                        // STS capability: different behavior for TLS vs plaintext
                        // Per spec: insecure gets port=, secure gets duration=
                        if let Some(sts) = sts_cfg {
                            if *is_tls {
                                // Secure connection: advertise persistence policy
                                let mut value = format!("sts=duration={}", sts.duration);
                                if sts.preload {
                                    value.push_str(",preload");
                                }
                                Some(value)
                            } else {
                                // Insecure connection: advertise upgrade policy
                                Some(format!("sts=port={}", sts.port))
                            }
                        } else {
                            None // STS not configured
                        }
                    }
                    _ => Some(cap.as_ref().to_string()),
                }
            } else {
                // For older CAP versions, do not advertise SASL on plaintext
                if *cap == Capability::Tls && *is_tls {
                    // Don't advertise tls (STARTTLS) on already-TLS connections
                    None
                } else if *cap == Capability::Sts {
                    // STS requires CAP 302+ for values
                    None
                } else if *cap == Capability::Sasl
                    && !*is_tls
                    && !sec_cfg.allow_plaintext_sasl_plain
                {
                    // For older clients, only advertise SASL on TLS connections
                    // unless plaintext is explicitly allowed.
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
    use crate::config::{AccountRegistrationConfig, SecurityConfig};

    // Helper to create CapListParams for tests
    fn make_params(version: u32, is_tls: bool, has_cert: bool) -> CapListParams<'static> {
        make_params_with_sec_cfg(version, is_tls, has_cert, SecurityConfig::default())
    }

    // Helper to create CapListParams with custom security config
    fn make_params_with_sec_cfg(
        version: u32,
        is_tls: bool,
        has_cert: bool,
        sec_cfg: SecurityConfig,
    ) -> CapListParams<'static> {
        // Use leaked boxes to get 'static lifetime for tests
        let acct_cfg: &'static AccountRegistrationConfig =
            Box::leak(Box::new(AccountRegistrationConfig::default()));
        let sec_cfg: &'static SecurityConfig = Box::leak(Box::new(sec_cfg));
        CapListParams {
            version,
            is_tls,
            has_cert,
            acct_cfg,
            sec_cfg,
            sts_cfg: None,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // build_cap_list_tokens tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_cap_list_plaintext_sasl_allowed() {
        // Test non-TLS SASL when allow_plaintext_sasl_plain is true (CAP 302)
        let mut sec_cfg = SecurityConfig::default();
        sec_cfg.allow_plaintext_sasl_plain = true;
        let caps = build_cap_list_tokens(&make_params_with_sec_cfg(302, false, false, sec_cfg));

        assert!(
            caps.iter().any(|c| c == "sasl=SCRAM-SHA-256,PLAIN"),
            "Plaintext with allow_plaintext_sasl_plain should advertise SASL: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_plaintext_sasl_allowed_301() {
        // Test non-TLS SASL when allow_plaintext_sasl_plain is true (CAP 301)
        let mut sec_cfg = SecurityConfig::default();
        sec_cfg.allow_plaintext_sasl_plain = true;
        let caps = build_cap_list_tokens(&make_params_with_sec_cfg(301, false, false, sec_cfg));

        assert!(
            caps.iter().any(|c| c == "sasl"),
            "Plaintext with allow_plaintext_sasl_plain should advertise SASL for CAP 301: {:?}",
            caps
        );
        assert!(
            !caps.iter().any(|c| c.contains('=')),
            "CAP 301 should not have capability values: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_tls_with_cert() {
        // TLS + cert → includes "sasl=SCRAM-SHA-256,PLAIN,EXTERNAL"
        let caps = build_cap_list_tokens(&make_params(302, true, true));

        assert!(
            caps.iter()
                .any(|c| c == "sasl=SCRAM-SHA-256,PLAIN,EXTERNAL"),
            "TLS with cert should advertise SASL SCRAM/PLAIN/EXTERNAL: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_tls_no_cert() {
        // TLS, no cert → includes "sasl=SCRAM-SHA-256,PLAIN" (not EXTERNAL)
        let caps = build_cap_list_tokens(&make_params(302, true, false));

        assert!(
            caps.iter().any(|c| c == "sasl=SCRAM-SHA-256,PLAIN"),
            "TLS without cert should advertise SASL SCRAM/PLAIN only: {:?}",
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
        let caps = build_cap_list_tokens(&make_params(302, false, false));

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
        let caps = build_cap_list_tokens(&make_params(302, true, false));

        assert!(
            !caps.iter().any(|c| c == "tls"),
            "TLS connection should NOT advertise STARTTLS: {:?}",
            caps
        );
    }

    #[test]
    fn test_cap_list_version_301_no_values() {
        // CAP version 301 → no values (just capability names)
        let caps = build_cap_list_tokens(&make_params(301, true, true));

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
        let caps = build_cap_list_tokens(&make_params(302, true, true));

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
        let caps = build_cap_list_tokens(&make_params(302, true, false));

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
        let caps = build_cap_list_tokens(&make_params(302, true, false));

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

    #[test]
    fn test_cap_list_sts_secure_connection() {
        // STS on secure connection → advertises duration
        let acct_cfg: &'static AccountRegistrationConfig =
            Box::leak(Box::new(AccountRegistrationConfig::default()));
        let sec_cfg: &'static SecurityConfig = Box::leak(Box::new(SecurityConfig::default()));
        let sts_cfg: &'static StsConfig = Box::leak(Box::new(StsConfig {
            port: 6697,
            duration: 2592000,
            preload: false,
        }));
        let caps = build_cap_list_tokens(&CapListParams {
            version: 302,
            is_tls: true,
            has_cert: false,
            acct_cfg,
            sec_cfg,
            sts_cfg: Some(sts_cfg),
        });

        let sts = caps
            .iter()
            .find(|c| c.starts_with("sts="))
            .expect("Should have STS cap on TLS");
        assert!(
            sts.contains("duration=2592000"),
            "STS should have duration on TLS: {}",
            sts
        );
        assert!(
            !sts.contains("port="),
            "STS should NOT have port on TLS: {}",
            sts
        );
    }

    #[test]
    fn test_cap_list_sts_insecure_connection() {
        // STS on insecure connection → advertises port for upgrade
        let acct_cfg: &'static AccountRegistrationConfig =
            Box::leak(Box::new(AccountRegistrationConfig::default()));
        let sec_cfg: &'static SecurityConfig = Box::leak(Box::new(SecurityConfig::default()));
        let sts_cfg: &'static StsConfig = Box::leak(Box::new(StsConfig {
            port: 6697,
            duration: 2592000,
            preload: false,
        }));
        let caps = build_cap_list_tokens(&CapListParams {
            version: 302,
            is_tls: false,
            has_cert: false,
            acct_cfg,
            sec_cfg,
            sts_cfg: Some(sts_cfg),
        });

        let sts = caps
            .iter()
            .find(|c| c.starts_with("sts="))
            .expect("Should have STS cap on plaintext");
        assert!(
            sts.contains("port=6697"),
            "STS should have port on plaintext: {}",
            sts
        );
        assert!(
            !sts.contains("duration="),
            "STS should NOT have duration on plaintext: {}",
            sts
        );
    }

    #[test]
    fn test_cap_list_sts_preload() {
        // STS with preload flag on secure connection
        let acct_cfg: &'static AccountRegistrationConfig =
            Box::leak(Box::new(AccountRegistrationConfig::default()));
        let sec_cfg: &'static SecurityConfig = Box::leak(Box::new(SecurityConfig::default()));
        let sts_cfg: &'static StsConfig = Box::leak(Box::new(StsConfig {
            port: 6697,
            duration: 31536000,
            preload: true,
        }));
        let caps = build_cap_list_tokens(&CapListParams {
            version: 302,
            is_tls: true,
            has_cert: false,
            acct_cfg,
            sec_cfg,
            sts_cfg: Some(sts_cfg),
        });

        let sts = caps
            .iter()
            .find(|c| c.starts_with("sts="))
            .expect("Should have STS cap");
        assert!(
            sts.contains("preload"),
            "STS should have preload flag: {}",
            sts
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
            assert!(combined.contains(cap), "Missing cap {} in output", cap);
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
