//! IRCv3 capability registry and definitions.

/// Definition of a known IRCv3 capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDef {
    /// Capability name (e.g., "multi-prefix")
    pub name: &'static str,
    /// Minimum CAP version that supports this capability (301 or 302)
    pub version: u32,
    /// Default value for capabilities that take parameters
    pub value: Option<&'static str>,
    /// Human-readable description
    pub description: &'static str,
}

/// Static list of supported capabilities.
pub const CAPABILITIES: &[CapabilityDef] = &[
    // CAP 3.1 capabilities
    CapabilityDef {
        name: "multi-prefix",
        version: 301,
        value: None,
        description: "Show all user modes in NAMES (@+nick for op+voice)",
    },
    CapabilityDef {
        name: "userhost-in-names",
        version: 301,
        value: None,
        description: "Include full nick!user@host in NAMES replies",
    },
    CapabilityDef {
        name: "away-notify",
        version: 301,
        value: None,
        description: "Broadcast AWAY status changes to channel members",
    },
    CapabilityDef {
        name: "account-notify",
        version: 301,
        value: None,
        description: "Account tag on messages + ACCOUNT command for login/logout",
    },
    CapabilityDef {
        name: "extended-join",
        version: 301,
        value: None,
        description: "JOIN includes account + realname",
    },
    CapabilityDef {
        name: "sasl",
        version: 301,
        value: Some("PLAIN"),
        description: "SASL authentication (PLAIN mechanism)",
    },
    CapabilityDef {
        name: "monitor",
        version: 301,
        value: None,
        description: "Online/offline status tracking",
    },
    // CAP 3.2 capabilities
    CapabilityDef {
        name: "account-tag",
        version: 302,
        value: None,
        description: "Add account tag to messages from logged-in users",
    },
    CapabilityDef {
        name: "echo-message",
        version: 302,
        value: None,
        description: "Send copy of PRIVMSG/NOTICE back to sender",
    },
    CapabilityDef {
        name: "server-time",
        version: 302,
        value: None,
        description: "Add time tag to messages (ISO 8601)",
    },
    CapabilityDef {
        name: "message-tags",
        version: 302,
        value: None,
        description: "Parse client tags from incoming messages",
    },
    CapabilityDef {
        name: "msgid",
        version: 302,
        value: None,
        description: "Unique message IDs for deduplication",
    },
    CapabilityDef {
        name: "labeled-response",
        version: 302,
        value: None,
        description: "Echo label tag for request/response correlation",
    },
    CapabilityDef {
        name: "batch",
        version: 302,
        value: None,
        description: "Multi-line response grouping",
    },
    CapabilityDef {
        name: "cap-notify",
        version: 302,
        value: None,
        description: "Server notifies clients of capability changes (CAP NEW/DEL)",
    },
    CapabilityDef {
        name: "chghost",
        version: 302,
        value: None,
        description: "Notify when user's hostname changes (CHGHOST command)",
    },
    CapabilityDef {
        name: "invite-notify",
        version: 302,
        value: None,
        description: "Notify channel members when someone is invited",
    },
    CapabilityDef {
        name: "setname",
        version: 302,
        value: None,
        description: "Change realname with SETNAME command",
    },
    CapabilityDef {
        name: "standard-replies",
        version: 302,
        value: None,
        description: "Machine-parseable FAIL/WARN/NOTE responses",
    },
    CapabilityDef {
        name: "sts",
        version: 302,
        value: Some("port=6697,duration=2592000"),
        description: "Strict Transport Security - upgrade plaintext to TLS",
    },
    // Draft/experimental capabilities
    CapabilityDef {
        name: "draft/chathistory",
        version: 302,
        value: None,
        description: "Chat history retrieval via CHATHISTORY command",
    },
    CapabilityDef {
        name: "draft/multiline",
        version: 302,
        value: Some("max-bytes=4096,max-lines=100"),
        description: "Multi-line message batches",
    },
    CapabilityDef {
        name: "draft/read-marker",
        version: 302,
        value: None,
        description: "Read marker synchronization across clients",
    },
    CapabilityDef {
        name: "typing",
        version: 302,
        value: None,
        description: "Typing notifications (+typing tag)",
    },
    CapabilityDef {
        name: "draft/event-playback",
        version: 302,
        value: None,
        description: "Include non-message events in history playback",
    },
    CapabilityDef {
        name: "draft/message-redaction",
        version: 302,
        value: None,
        description: "Message deletion/redaction support",
    },
];

/// Build a space-separated list of capabilities for CAP LS response.
///
/// # Arguments
/// * `version` - CAP version (301 or 302)
/// * `tls_port` - Optional TLS port for STS capability
pub fn get_cap_list(version: u32, tls_port: Option<u16>) -> String {
    let caps: Vec<String> = CAPABILITIES
        .iter()
        .filter(|cap| {
            // Only include STS if we have a TLS port
            if cap.name == "sts" && tls_port.is_none() {
                return false;
            }
            cap.version <= version
        })
        .map(|cap| {
            if version >= 302 && cap.value.is_some() {
                if cap.name == "sts" {
                    if let Some(port) = tls_port {
                        format!("{}=port={},duration=2592000", cap.name, port)
                    } else {
                        cap.name.to_string()
                    }
                } else if let Some(value) = cap.value {
                    format!("{}={}", cap.name, value)
                } else {
                    cap.name.to_string()
                }
            } else {
                cap.name.to_string()
            }
        })
        .collect();

    caps.join(" ")
}

/// Check if a capability name is supported.
pub fn is_supported(name: &str) -> bool {
    CAPABILITIES.iter().any(|cap| cap.name == name)
}

/// Get all supported capability names.
pub fn get_all_names() -> Vec<&'static str> {
    CAPABILITIES.iter().map(|cap| cap.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        assert!(is_supported("multi-prefix"));
        assert!(is_supported("sasl"));
        assert!(!is_supported("unknown-capability"));
    }

    #[test]
    fn test_cap_list_v301() {
        let list = get_cap_list(301, None);
        assert!(list.contains("multi-prefix"));
        assert!(!list.contains("echo-message")); // v302 only
        assert!(!list.contains("sts")); // needs TLS port
    }

    #[test]
    fn test_cap_list_v302() {
        let list = get_cap_list(302, Some(6697));
        assert!(list.contains("multi-prefix"));
        assert!(list.contains("echo-message"));
        assert!(list.contains("sts=port=6697"));
    }

    #[test]
    fn test_draft_capabilities_supported() {
        assert!(is_supported("draft/chathistory"));
        assert!(is_supported("draft/multiline"));
        assert!(is_supported("draft/read-marker"));
        assert!(is_supported("typing"));
        assert!(is_supported("draft/event-playback"));
        assert!(is_supported("draft/message-redaction"));
    }

    #[test]
    fn test_cap_list_includes_draft() {
        let list = get_cap_list(302, None);
        assert!(list.contains("draft/chathistory"));
        assert!(list.contains("typing"));
        // multiline has a value
        assert!(list.contains("draft/multiline=max-bytes=4096,max-lines=100"));
    }
}
