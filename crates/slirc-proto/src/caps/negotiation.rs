//! CAP protocol negotiation functions.

use std::collections::HashSet;

use super::registry::is_supported;

/// Parse a CAP REQ request and separate accepted from rejected capabilities.
///
/// Returns (accepted, rejected) capability lists.
pub fn parse_request(requested: &str) -> (Vec<String>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();

    for cap in requested.split_whitespace() {
        // Check for removal prefix
        let (is_removal, cap_name) = if let Some(name) = cap.strip_prefix('-') {
            (true, name)
        } else {
            (false, cap)
        };

        // Strip any value suffix (cap=value)
        let cap_base = cap_name.split('=').next().unwrap_or(cap_name);

        if is_supported(cap_base) {
            accepted.push(if is_removal {
                format!("-{}", cap_base)
            } else {
                cap_base.to_string()
            });
        } else {
            rejected.push(cap_base.to_string());
        }
    }

    (accepted, rejected)
}

/// Apply capability changes to an active set.
///
/// Changes prefixed with '-' remove capabilities, others add them.
/// Returns true if any changes were made.
pub fn apply_changes(capabilities: &mut HashSet<String>, changes: &[String]) -> bool {
    let mut modified = false;

    for change in changes {
        if let Some(cap_name) = change.strip_prefix('-') {
            if capabilities.remove(cap_name) {
                modified = true;
            }
        } else if capabilities.insert(change.clone()) {
            modified = true;
        }
    }

    modified
}

/// Format a CAP NEW message for notifying clients of new capabilities.
pub fn format_cap_new(nickname: &str, server_name: &str, new_caps: &[&str]) -> String {
    format!(
        ":{} CAP {} NEW :{}",
        server_name,
        nickname,
        new_caps.join(" ")
    )
}

/// Format a CAP DEL message for notifying clients of removed capabilities.
pub fn format_cap_del(nickname: &str, server_name: &str, removed_caps: &[&str]) -> String {
    format!(
        ":{} CAP {} DEL :{}",
        server_name,
        nickname,
        removed_caps.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let (accepted, rejected) = parse_request("multi-prefix sasl unknown-cap");
        assert!(accepted.contains(&"multi-prefix".to_string()));
        assert!(accepted.contains(&"sasl".to_string()));
        assert!(rejected.contains(&"unknown-cap".to_string()));
    }

    #[test]
    fn test_parse_request_removal() {
        let (accepted, _) = parse_request("-multi-prefix");
        assert!(accepted.contains(&"-multi-prefix".to_string()));
    }

    #[test]
    fn test_apply_changes() {
        let mut caps = HashSet::new();

        let changes = vec!["multi-prefix".to_string(), "sasl".to_string()];
        assert!(apply_changes(&mut caps, &changes));
        assert!(caps.contains("multi-prefix"));
        assert!(caps.contains("sasl"));

        let removal = vec!["-sasl".to_string()];
        assert!(apply_changes(&mut caps, &removal));
        assert!(!caps.contains("sasl"));
    }
}
