//! IRCv3 Message Tag Compliance Tests
//!
//! Tests for edge cases in tag escaping/unescaping as per the IRCv3
//! message-tags specification: https://ircv3.net/specs/extensions/message-tags

use slirc_proto::Message;

// Helper to find a tag by key (Tag is a tuple struct: Tag(key, value))
fn find_tag<'a>(tags: &'a [slirc_proto::Tag], key: &str) -> Option<&'a slirc_proto::Tag> {
    tags.iter().find(|t| t.0.as_ref() == key)
}

// =============================================================================
// Tag Parsing Edge Cases
// =============================================================================

#[test]
fn test_empty_tag_value() {
    // Tags can have empty values: @key= or just @key (no value at all)
    let raw = "@empty= :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse empty tag value");

    // Empty value should be Some("") not None
    if let Some(tags) = &msg.tags {
        let empty_tag = find_tag(tags, "empty");
        assert!(empty_tag.is_some(), "Should have 'empty' tag");
        assert_eq!(empty_tag.unwrap().1, Some("".to_string()));
    }
}

#[test]
fn test_tag_with_no_value() {
    // Tags without = are valid (boolean flags)
    let raw = "@flag :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse boolean tag");

    if let Some(tags) = &msg.tags {
        let flag_tag = find_tag(tags, "flag");
        assert!(flag_tag.is_some(), "Should have 'flag' tag");
        assert_eq!(flag_tag.unwrap().1, None);
    }
}

#[test]
fn test_only_escape_sequences() {
    // Value consisting entirely of escape sequences
    // \s\:\r\n should become " ;\r\n"
    let raw = "@escapes=\\s\\:\\r\\n :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse escape-only value");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "escapes");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some(" ;\r\n".to_string()));
    }
}

#[test]
fn test_trailing_backslash() {
    // Trailing backslash with no following character
    // Per IRCv3: "If a backslash is not followed by a recognized escape character,
    // it is simply ignored (the backslash is removed)."
    let raw = "@trailing=value\\ :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse trailing backslash");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "trailing");
        assert!(tag.is_some());
        // Trailing backslash should be dropped
        assert_eq!(tag.unwrap().1, Some("value".to_string()));
    }
}

#[test]
fn test_invalid_escape_sequences() {
    // Unknown escape like \a should become just 'a' (backslash dropped)
    let raw = "@invalid=\\a\\b\\c :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse invalid escapes");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "invalid");
        assert!(tag.is_some());
        // Unknown escapes: backslash is dropped, char is kept
        assert_eq!(tag.unwrap().1, Some("abc".to_string()));
    }
}

#[test]
fn test_escaped_backslash_before_escape_char() {
    // \\s should become \s (escaped backslash followed by 's', not a space)
    // Wait no: \\\s = \\ + \s = \ + space
    // And \\s = \\ followed by literal s = \s
    let raw = "@double=\\\\s :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse double backslash");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "double");
        assert!(tag.is_some());
        // \\\\ = one backslash, s = literal 's'
        assert_eq!(tag.unwrap().1, Some("\\s".to_string()));
    }
}

#[test]
fn test_multiple_consecutive_escapes() {
    // Multiple consecutive escape sequences
    let raw = "@multi=\\s\\s\\s :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse multiple escapes");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "multi");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("   ".to_string()));
    }
}

#[test]
fn test_escaped_semicolon_in_value() {
    // Semicolon must be escaped with \: or it terminates the tag
    // Note: \: escapes semicolon (;), not colon (:) per IRCv3 spec
    // Colons don't need escaping in tag values
    let raw = "@semi=has\\:semicolon :server PING :test\r\n";
    let msg: Message = raw.parse().expect("Should parse escaped semicolon");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "semi");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("has;semicolon".to_string()));
    }
}

// =============================================================================
// Tag Serialization Round-Trip Tests
// =============================================================================

#[test]
fn test_roundtrip_special_characters() {
    // Create a message with special characters in tag value
    let msg = Message::privmsg("#test", "hello").with_tag("data", Some("has;semi and\\back"));

    let serialized = msg.to_string();
    let parsed: Message = serialized.parse().expect("Should re-parse");

    if let Some(tags) = &parsed.tags {
        let tag = find_tag(tags, "data");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("has;semi and\\back".to_string()));
    }
}

#[test]
fn test_roundtrip_spaces_in_value() {
    let msg = Message::privmsg("#test", "hello").with_tag("phrase", Some("hello world"));

    let serialized = msg.to_string();
    let parsed: Message = serialized.parse().expect("Should re-parse");

    if let Some(tags) = &parsed.tags {
        let tag = find_tag(tags, "phrase");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("hello world".to_string()));
    }
}

#[test]
fn test_roundtrip_newlines_in_value() {
    let msg = Message::privmsg("#test", "hello").with_tag("multiline", Some("line1\nline2\rline3"));

    let serialized = msg.to_string();
    let parsed: Message = serialized.parse().expect("Should re-parse");

    if let Some(tags) = &parsed.tags {
        let tag = find_tag(tags, "multiline");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("line1\nline2\rline3".to_string()));
    }
}

// =============================================================================
// Multiple Tags Tests
// =============================================================================

#[test]
fn test_multiple_tags_with_escapes() {
    // Note: \: escapes semicolon (;), colons don't need escaping
    let raw = "@time=12:00;msgid=abc\\sdef;flag :server PRIVMSG #chan :hi\r\n";
    let msg: Message = raw.parse().expect("Should parse multiple tags");

    if let Some(tags) = &msg.tags {
        assert!(tags
            .iter()
            .any(|t| t.0.as_ref() == "time" && t.1 == Some("12:00".to_string())));
        assert!(tags
            .iter()
            .any(|t| t.0.as_ref() == "msgid" && t.1 == Some("abc def".to_string())));
        assert!(tags.iter().any(|t| t.0.as_ref() == "flag" && t.1.is_none()));
    }
}

#[test]
fn test_vendor_prefixed_tags() {
    // Vendor-prefixed tags like example.com/key
    let raw = "@example.com/custom=value;+draft/reply=target :server PRIVMSG #chan :hi\r\n";
    let msg: Message = raw.parse().expect("Should parse vendor tags");

    if let Some(tags) = &msg.tags {
        assert!(tags.iter().any(|t| t.0.as_ref() == "example.com/custom"));
        assert!(tags.iter().any(|t| t.0.as_ref() == "+draft/reply"));
    }
}

#[test]
fn test_client_only_tag_prefix() {
    // Client-only tags start with +
    let raw = "@+typing=active :nick!user@host TAGMSG #channel\r\n";
    let msg: Message = raw.parse().expect("Should parse client-only tag");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "+typing");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1, Some("active".to_string()));
    }
}

// =============================================================================
// Edge Cases That Should NOT Crash
// =============================================================================

#[test]
fn test_empty_tag_section() {
    // Just @ with nothing after - malformed but shouldn't crash
    let raw = "@ :server PING :test\r\n";
    // This may fail to parse, but should not panic
    let _ = raw.parse::<Message>();
}

#[test]
fn test_tag_with_only_equals() {
    // @= is malformed
    let raw = "@= :server PING :test\r\n";
    let _ = raw.parse::<Message>();
}

#[test]
fn test_very_long_tag_value() {
    // Tags can be up to 8191 bytes total (including all tags)
    let long_value = "x".repeat(1000);
    let raw = format!("@data={} :server PING :test\r\n", long_value);
    let msg: Message = raw.parse().expect("Should parse long tag");

    if let Some(tags) = &msg.tags {
        let tag = find_tag(tags, "data");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().1.as_ref().map(|s| s.len()), Some(1000));
    }
}
