//! Common utilities for channel command handlers.
//!
//! This module contains pure parsing and validation functions shared across
//! JOIN, PART, KICK, and other channel commands.

/// Parse a comma-separated list of channel names.
/// Returns a vector of trimmed, non-empty channel name strings.
///
/// # Examples
/// ```ignore
/// let channels = parse_channel_list("#foo,#bar,#baz");
/// assert_eq!(channels, vec!["#foo", "#bar", "#baz"]);
///
/// let with_spaces = parse_channel_list(" #foo , #bar ");
/// assert_eq!(with_spaces, vec!["#foo", "#bar"]);
///
/// let with_empty = parse_channel_list("#foo,,#bar");
/// assert_eq!(with_empty, vec!["#foo", "#bar"]);
/// ```
pub fn parse_channel_list(channels_str: &str) -> Vec<&str> {
    channels_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a comma-separated list of keys and align with channel count.
/// Empty keys are represented as None, missing keys are padded with None.
///
/// # Examples
/// ```ignore
/// let keys = parse_key_list(Some("key1,key2"), 3);
/// assert_eq!(keys, vec![Some("key1"), Some("key2"), None]);
/// ```
pub fn parse_key_list(keys_str: Option<&str>, channel_count: usize) -> Vec<Option<&str>> {
    if let Some(keys) = keys_str {
        let mut key_list: Vec<Option<&str>> = keys
            .split(',')
            .map(|k| {
                let trimmed = k.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .collect();
        key_list.resize(channel_count, None);
        key_list
    } else {
        vec![None; channel_count]
    }
}

/// Parse an optional part/quit reason message.
/// Returns None if the reason is empty or whitespace-only.
pub fn parse_reason(reason: Option<&str>) -> Option<&str> {
    reason.map(|r| r.trim()).filter(|r| !r.is_empty())
}

/// Check if JOIN 0 syntax (leave all channels).
#[inline]
pub fn is_join_zero(channels_str: &str) -> bool {
    channels_str == "0"
}

/// Parse a comma-separated list of nicknames.
/// Returns a vector of trimmed, non-empty nick strings.
/// Used by KICK for multi-target kicks.
///
/// # Examples
/// ```ignore
/// let nicks = parse_nick_list("alice,bob,charlie");
/// assert_eq!(nicks, vec!["alice", "bob", "charlie"]);
/// ```
pub fn parse_nick_list(nicks_str: &str) -> Vec<&str> {
    nicks_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Build channel:target pairs for KICK command.
///
/// RFC 2812 supports two modes:
/// 1. Single channel with multiple targets: `KICK #chan nick1,nick2,nick3`
/// 2. Equal channel:nick pairs: `KICK #chan1,#chan2 nick1,nick2`
///
/// This function handles both cases:
/// - If 1 channel, pairs it with all targets
/// - If equal counts, pairs them 1:1
/// - If mismatched counts, pairs as many as possible (ignores extras)
///
/// Returns empty vec if either list is empty.
pub fn build_kick_pairs<'a>(channels: &[&'a str], targets: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    if channels.is_empty() || targets.is_empty() {
        return Vec::new();
    }

    if channels.len() == 1 {
        // Single channel with multiple targets
        targets
            .iter()
            .filter(|t| !t.is_empty())
            .map(|&target| (channels[0], target))
            .collect()
    } else {
        // Pair channels with targets (1:1 or mismatched)
        channels
            .iter()
            .zip(targets.iter())
            .filter(|(c, t)| !c.is_empty() && !t.is_empty())
            .map(|(&c, &t)| (c, t))
            .collect()
    }
}

/// Get a default kick reason (kicker's nick) if none provided.
/// Returns the provided reason trimmed, or the default if empty/None.
pub fn kick_reason_or_default<'a>(reason: Option<&'a str>, kicker_nick: &'a str) -> &'a str {
    match reason {
        Some(r) if !r.trim().is_empty() => r.trim(),
        _ => kicker_nick,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Channel list parsing tests
    // ========================================================================

    #[test]
    fn test_parse_single_channel() {
        let channels = parse_channel_list("#test");
        assert_eq!(channels, vec!["#test"]);
    }

    #[test]
    fn test_parse_multiple_channels() {
        let channels = parse_channel_list("#foo,#bar,#baz");
        assert_eq!(channels, vec!["#foo", "#bar", "#baz"]);
    }

    #[test]
    fn test_parse_channels_with_whitespace() {
        let channels = parse_channel_list(" #foo , #bar , #baz ");
        assert_eq!(channels, vec!["#foo", "#bar", "#baz"]);
    }

    #[test]
    fn test_parse_channels_with_empty_entries() {
        let channels = parse_channel_list("#foo,,#bar");
        assert_eq!(channels, vec!["#foo", "#bar"]);
    }

    #[test]
    fn test_parse_empty_channel_list() {
        let channels = parse_channel_list("");
        assert!(channels.is_empty());
    }

    #[test]
    fn test_parse_only_commas() {
        let channels = parse_channel_list(",,,");
        assert!(channels.is_empty());
    }

    // ========================================================================
    // Key list parsing tests
    // ========================================================================

    #[test]
    fn test_parse_single_key() {
        let keys = parse_key_list(Some("secret"), 1);
        assert_eq!(keys, vec![Some("secret")]);
    }

    #[test]
    fn test_parse_multiple_keys() {
        let keys = parse_key_list(Some("key1,key2,key3"), 3);
        assert_eq!(keys, vec![Some("key1"), Some("key2"), Some("key3")]);
    }

    #[test]
    fn test_parse_keys_with_padding() {
        // 3 channels but only 1 key - should pad with None
        let keys = parse_key_list(Some("secret"), 3);
        assert_eq!(keys, vec![Some("secret"), None, None]);
    }

    #[test]
    fn test_parse_keys_with_empty_entries() {
        let keys = parse_key_list(Some("key1,,key3"), 3);
        assert_eq!(keys, vec![Some("key1"), None, Some("key3")]);
    }

    #[test]
    fn test_parse_no_keys() {
        let keys = parse_key_list(None, 3);
        assert_eq!(keys, vec![None, None, None]);
    }

    #[test]
    fn test_parse_keys_truncated() {
        // More keys than channels - resize truncates
        let keys = parse_key_list(Some("key1,key2,key3"), 2);
        assert_eq!(keys, vec![Some("key1"), Some("key2")]);
    }

    // ========================================================================
    // Reason parsing tests
    // ========================================================================

    #[test]
    fn test_parse_reason_some() {
        assert_eq!(parse_reason(Some("Goodbye!")), Some("Goodbye!"));
    }

    #[test]
    fn test_parse_reason_none() {
        assert_eq!(parse_reason(None), None);
    }

    #[test]
    fn test_parse_reason_empty() {
        assert_eq!(parse_reason(Some("")), None);
    }

    #[test]
    fn test_parse_reason_whitespace_only() {
        assert_eq!(parse_reason(Some("   ")), None);
    }

    #[test]
    fn test_parse_reason_with_whitespace() {
        assert_eq!(parse_reason(Some("  Leaving now  ")), Some("Leaving now"));
    }

    // ========================================================================
    // JOIN 0 detection tests
    // ========================================================================

    #[test]
    fn test_is_join_zero_true() {
        assert!(is_join_zero("0"));
    }

    #[test]
    fn test_is_join_zero_false() {
        assert!(!is_join_zero("#test"));
        assert!(!is_join_zero("00"));
        assert!(!is_join_zero(" 0"));
        assert!(!is_join_zero("0 "));
    }

    // ========================================================================
    // Nick list parsing tests (for KICK)
    // ========================================================================

    #[test]
    fn test_parse_single_nick() {
        let nicks = parse_nick_list("alice");
        assert_eq!(nicks, vec!["alice"]);
    }

    #[test]
    fn test_parse_multiple_nicks() {
        let nicks = parse_nick_list("alice,bob,charlie");
        assert_eq!(nicks, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn test_parse_nicks_with_whitespace() {
        let nicks = parse_nick_list(" alice , bob , charlie ");
        assert_eq!(nicks, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn test_parse_nicks_with_empty_entries() {
        let nicks = parse_nick_list("alice,,bob");
        assert_eq!(nicks, vec!["alice", "bob"]);
    }

    #[test]
    fn test_parse_empty_nick_list() {
        let nicks = parse_nick_list("");
        assert!(nicks.is_empty());
    }

    // ========================================================================
    // KICK pairs building tests
    // ========================================================================

    #[test]
    fn test_kick_pairs_single_channel_multiple_targets() {
        let channels = vec!["#test"];
        let targets = vec!["alice", "bob", "charlie"];
        let pairs = build_kick_pairs(&channels, &targets);
        assert_eq!(
            pairs,
            vec![("#test", "alice"), ("#test", "bob"), ("#test", "charlie")]
        );
    }

    #[test]
    fn test_kick_pairs_equal_counts() {
        let channels = vec!["#foo", "#bar", "#baz"];
        let targets = vec!["alice", "bob", "charlie"];
        let pairs = build_kick_pairs(&channels, &targets);
        assert_eq!(
            pairs,
            vec![("#foo", "alice"), ("#bar", "bob"), ("#baz", "charlie")]
        );
    }

    #[test]
    fn test_kick_pairs_more_channels_than_targets() {
        let channels = vec!["#foo", "#bar", "#baz"];
        let targets = vec!["alice"];
        let pairs = build_kick_pairs(&channels, &targets);
        // Only pairs what's available
        assert_eq!(pairs, vec![("#foo", "alice")]);
    }

    #[test]
    fn test_kick_pairs_more_targets_than_channels() {
        let channels = vec!["#foo"];
        let targets = vec!["alice", "bob", "charlie"];
        // Single channel mode - all targets get paired
        let pairs = build_kick_pairs(&channels, &targets);
        assert_eq!(
            pairs,
            vec![("#foo", "alice"), ("#foo", "bob"), ("#foo", "charlie")]
        );
    }

    #[test]
    fn test_kick_pairs_two_channels_three_targets() {
        // Multi-channel mode (not single), pairs 1:1
        let channels = vec!["#foo", "#bar"];
        let targets = vec!["alice", "bob", "charlie"];
        let pairs = build_kick_pairs(&channels, &targets);
        // Only first two pairs
        assert_eq!(pairs, vec![("#foo", "alice"), ("#bar", "bob")]);
    }

    #[test]
    fn test_kick_pairs_empty_channels() {
        let channels: Vec<&str> = vec![];
        let targets = vec!["alice"];
        let pairs = build_kick_pairs(&channels, &targets);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_kick_pairs_empty_targets() {
        let channels = vec!["#test"];
        let targets: Vec<&str> = vec![];
        let pairs = build_kick_pairs(&channels, &targets);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_kick_pairs_filters_empty_strings() {
        let channels = vec!["#test"];
        let targets = vec!["alice", "", "bob"];
        let pairs = build_kick_pairs(&channels, &targets);
        assert_eq!(pairs, vec![("#test", "alice"), ("#test", "bob")]);
    }

    // ========================================================================
    // Kick reason default tests
    // ========================================================================

    #[test]
    fn test_kick_reason_provided() {
        assert_eq!(kick_reason_or_default(Some("spamming"), "kicker"), "spamming");
    }

    #[test]
    fn test_kick_reason_none_uses_default() {
        assert_eq!(kick_reason_or_default(None, "kicker"), "kicker");
    }

    #[test]
    fn test_kick_reason_empty_uses_default() {
        assert_eq!(kick_reason_or_default(Some(""), "kicker"), "kicker");
    }

    #[test]
    fn test_kick_reason_whitespace_uses_default() {
        assert_eq!(kick_reason_or_default(Some("   "), "kicker"), "kicker");
    }

    #[test]
    fn test_kick_reason_trims_whitespace() {
        assert_eq!(
            kick_reason_or_default(Some("  bad behavior  "), "kicker"),
            "bad behavior"
        );
    }
}
