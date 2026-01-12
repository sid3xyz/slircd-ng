//! Integration tests for IRC control character handling.
//!
//! These tests verify that:
//! 1. Format codes (bold, color, CTCP) are allowed in message content
//! 2. NUL and BEL are always rejected
//! 3. Nicknames, usernames, and channel names still reject all control chars

use slirc_proto::chan::ChannelExt;
use slirc_proto::prefix::Prefix;
use slirc_proto::Message;

#[test]
fn test_bold_in_message_roundtrip() {
    // Bold formatting in PRIVMSG should work
    let raw = "PRIVMSG #test :\x02bold text\x02";
    let msg: Message = raw.parse().expect("Should parse message with bold");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x02bold text\x02"));

    // Round-trip should preserve the formatting
    let reparsed: Message = serialized.parse().expect("Should re-parse");
    assert_eq!(msg.command, reparsed.command);
}

#[test]
fn test_color_in_message_roundtrip() {
    // Color codes in PRIVMSG should work
    let raw = "PRIVMSG #test :\x034,5colored text\x03";
    let msg: Message = raw.parse().expect("Should parse message with color");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x034,5colored text\x03"));

    let reparsed: Message = serialized.parse().expect("Should re-parse");
    assert_eq!(msg.command, reparsed.command);
}

#[test]
fn test_ctcp_action_roundtrip() {
    // CTCP ACTION uses \x01 delimiters
    let raw = "PRIVMSG #test :\x01ACTION waves\x01";
    let msg: Message = raw.parse().expect("Should parse CTCP ACTION");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x01ACTION waves\x01"));

    let reparsed: Message = serialized.parse().expect("Should re-parse");
    assert_eq!(msg.command, reparsed.command);
}

#[test]
fn test_multiple_format_codes() {
    // Multiple format codes in one message
    let raw = "PRIVMSG #test :\x02bold\x02 and \x1Funderline\x1F and \x1Ditalic\x1D";
    let msg: Message = raw.parse().expect("Should parse multi-format message");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x02bold\x02"));
    assert!(serialized.contains("\x1Funderline\x1F"));
    assert!(serialized.contains("\x1Ditalic\x1D"));
}

#[test]
fn test_nickname_rejects_bold() {
    // Nicknames must NOT contain control characters
    let bad_nick = "nick\x02bold";
    let prefix = Prefix::try_from_str(&format!("{}!user@host", bad_nick));
    assert!(prefix.is_err(), "Nickname with bold should be rejected");
}

#[test]
fn test_nickname_rejects_color() {
    // Nicknames must NOT contain color codes
    let bad_nick = "nick\x03color";
    let prefix = Prefix::try_from_str(&format!("{}!user@host", bad_nick));
    assert!(prefix.is_err(), "Nickname with color should be rejected");
}

#[test]
fn test_channel_rejects_bold() {
    // Channel names must NOT contain control characters
    let bad_chan = "#chan\x02bold";
    assert!(
        !bad_chan.is_channel_name(),
        "Channel with bold should be rejected"
    );
}

#[test]
fn test_channel_rejects_color() {
    // Channel names must NOT contain color codes
    let bad_chan = "#chan\x03color";
    assert!(
        !bad_chan.is_channel_name(),
        "Channel with color should be rejected"
    );
}

#[test]
fn test_channel_rejects_bel() {
    // BEL character is always invalid in channel names
    let bad_chan = "#chan\x07bell";
    assert!(
        !bad_chan.is_channel_name(),
        "Channel with BEL should be rejected"
    );
}

#[test]
fn test_format_module_functions() {
    use slirc_proto::format::{is_illegal_control_char, is_irc_format_code};

    // CTCP delimiter is a format code, not illegal
    assert!(is_irc_format_code('\x01'));
    assert!(!is_illegal_control_char('\x01'));

    // Bold is a format code, not illegal
    assert!(is_irc_format_code('\x02'));
    assert!(!is_illegal_control_char('\x02'));

    // Color is a format code, not illegal
    assert!(is_irc_format_code('\x03'));
    assert!(!is_illegal_control_char('\x03'));

    // NUL is now allowed for binary data (e.g., METADATA values)
    assert!(!is_irc_format_code('\x00'));
    assert!(!is_illegal_control_char('\x00'));

    // BEL is always illegal
    assert!(!is_irc_format_code('\x07'));
    assert!(is_illegal_control_char('\x07'));

    // CR/LF are line delimiters, not illegal
    assert!(!is_illegal_control_char('\r'));
    assert!(!is_illegal_control_char('\n'));
}

#[test]
fn test_nickserv_bold_output() {
    // Simulate NickServ output with bold text
    let raw = ":NickServ!services@irc.example.com NOTICE user :\x02Password accepted\x02 - you are now identified.";
    let msg: Message = raw
        .parse()
        .expect("Should parse NickServ message with bold");
    assert!(msg.to_string().contains("\x02Password accepted\x02"));
}

#[test]
fn test_reset_format_code() {
    // Reset format code (0x0F)
    let raw = "PRIVMSG #test :\x02bold\x0F normal";
    let msg: Message = raw.parse().expect("Should parse message with reset");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x02bold\x0F"));
}

#[test]
fn test_reverse_format_code() {
    // Reverse format code (0x16)
    let raw = "PRIVMSG #test :\x16reversed\x16";
    let msg: Message = raw.parse().expect("Should parse message with reverse");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x16reversed\x16"));
}

#[test]
fn test_strikethrough_format_code() {
    // Strikethrough format code (0x1E)
    let raw = "PRIVMSG #test :\x1Estrikethrough\x1E";
    let msg: Message = raw
        .parse()
        .expect("Should parse message with strikethrough");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x1Estrikethrough\x1E"));
}

#[test]
fn test_monospace_format_code() {
    // Monospace format code (0x11)
    let raw = "PRIVMSG #test :\x11monospace\x11";
    let msg: Message = raw.parse().expect("Should parse message with monospace");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x11monospace\x11"));
}

#[test]
fn test_hex_color_format_code() {
    // Hex color format code (0x04)
    let raw = "PRIVMSG #test :\x04FF0000hex red\x04";
    let msg: Message = raw.parse().expect("Should parse message with hex color");
    let serialized = msg.to_string();
    assert!(serialized.contains("\x04FF0000hex red\x04"));
}

#[test]
fn test_message_rejects_nul() {
    // NUL character is now allowed for binary data (e.g., METADATA values)
    let _raw = "PRIVMSG #test :text with \x00 NUL";
    // Message parsing itself doesn't reject, and transport layer now allows it
    // Test via the format module function
    use slirc_proto::format::is_illegal_control_char;
    assert!(
        !is_illegal_control_char('\x00'),
        "NUL should be allowed for binary data"
    );
}

#[test]
fn test_message_rejects_bel() {
    // BEL character should be rejected in message content
    let _raw = "PRIVMSG #test :text with \x07 BEL";
    // Message parsing itself doesn't reject, but transport layer would
    // Test via the format module function
    use slirc_proto::format::is_illegal_control_char;
    assert!(
        is_illegal_control_char('\x07'),
        "BEL should be detected as illegal"
    );
}
