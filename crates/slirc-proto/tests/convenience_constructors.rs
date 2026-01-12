//! Integration test demonstrating the convenience constructors
//!
//! These tests show how the new convenience constructors can be used
//! to create IRC messages more ergonomically.

use slirc_proto::{prefix::Prefix, Message};

#[test]
fn test_convenience_api_usage() {
    // Basic message construction
    let privmsg = Message::privmsg("#rust", "Hello, Rust developers!");
    assert!(privmsg
        .to_string()
        .contains("PRIVMSG #rust :Hello, Rust developers!"));

    // Message with tags
    let tagged_msg = Message::notice("user123", "Welcome to the server!")
        .with_tag("time", Some("2023-01-01T12:00:00Z"))
        .with_tag("server", Some("irc.example.com"));

    let serialized = tagged_msg.to_string();
    assert!(serialized.contains("@time=2023-01-01T12:00:00Z;server=irc.example.com"));
    assert!(serialized.contains("NOTICE user123 :Welcome to the server!"));

    // Complex message construction with all features
    let complex_msg = Message::privmsg("#general", "This is a test message")
        .with_tag("msgid", Some("abc123"))
        .with_tag("batch", Some("batch001"))
        .with_prefix(Prefix::new_from_str("testbot!bot@example.com"));

    let complex_serialized = complex_msg.to_string();
    assert!(complex_serialized.contains("@msgid=abc123;batch=batch001"));
    assert!(complex_serialized.contains(":testbot!bot@example.com"));
    assert!(complex_serialized.contains("PRIVMSG #general :This is a test message"));

    // IRC connection flow messages
    let nick = Message::nick("TestUser");
    let user = Message::user("testuser", "Test User Real Name");
    let join = Message::join("#welcome");
    let part = Message::part_with_message("#welcome", "Thanks for the chat!");
    let quit = Message::quit_with_message("Goodbye!");

    // All should parse and serialize correctly
    for msg in [&nick, &user, &join, &part, &quit] {
        let serialized = msg.to_string();
        let parsed: Message = serialized.parse().expect("Should parse successfully");
        assert_eq!(msg, &parsed, "Message should round-trip correctly");
    }
}

#[test]
fn test_message_builder_pattern() {
    // Demonstrate builder-pattern usage
    let message = Message::privmsg("#development", "Check out this new feature!")
        .with_tag("time", Some("2023-01-01T15:30:00Z"))
        .with_tag("msgid", Some("feature-123"))
        .with_tag("reply-to", Some("msg-456"))
        .with_prefix(Prefix::new_from_str("developer!dev@company.com"));

    // Verify the message contains all expected parts
    let serialized = message.to_string();

    // Should have all three tags
    assert!(serialized.contains("time=2023-01-01T15:30:00Z"));
    assert!(serialized.contains("msgid=feature-123"));
    assert!(serialized.contains("reply-to=msg-456"));

    // Should have the prefix
    assert!(serialized.contains(":developer!dev@company.com"));

    // Should have the command and parameters
    assert!(serialized.contains("PRIVMSG #development :Check out this new feature!"));

    // Should round-trip successfully
    let reparsed: Message = serialized.parse().expect("Should parse successfully");
    assert_eq!(message, reparsed);
}

#[test]
fn test_ping_pong_flow() {
    // Simulate a PING/PONG exchange
    let ping = Message::ping("irc.example.com");
    let ping_serialized = ping.to_string();

    // Parse the PING
    let _parsed_ping: Message = ping_serialized.parse().expect("PING should parse");

    // Generate appropriate PONG response
    let pong = Message::pong("irc.example.com");
    let pong_serialized = pong.to_string();

    // Both should contain the server
    assert!(ping_serialized.contains("irc.example.com"));
    assert!(pong_serialized.contains("irc.example.com"));

    // Should round-trip
    let parsed_pong: Message = pong_serialized.parse().expect("PONG should parse");
    assert_eq!(pong, parsed_pong);
}

#[test]
fn test_channel_management() {
    // Channel join/part scenarios
    let join_simple = Message::join("#testing");
    let join_with_key = Message::join_with_key("#private", "secret123");
    let part_simple = Message::part("#testing");
    let part_with_msg = Message::part_with_message("#testing", "Thanks for the help!");

    // Kick scenarios
    let kick_simple = Message::kick("#moderated", "spammer");
    let kick_with_reason = Message::kick_with_reason("#moderated", "spammer", "Posting spam");

    let messages = [
        &join_simple,
        &join_with_key,
        &part_simple,
        &part_with_msg,
        &kick_simple,
        &kick_with_reason,
    ];

    // All should serialize and parse correctly
    for msg in messages {
        let serialized = msg.to_string();
        let parsed: Message = serialized
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", serialized, e));
        assert_eq!(msg, &parsed, "Message should round-trip: {}", serialized);
    }
}

#[test]
fn test_away_status() {
    // Away status management
    let away_simple = Message::away();
    let away_with_msg = Message::away_with_message("Gone to lunch, back in 30 minutes");

    // Both should serialize correctly
    let away_serialized = away_simple.to_string();
    let away_msg_serialized = away_with_msg.to_string();

    assert!(away_serialized.contains("AWAY"));
    assert!(away_msg_serialized.contains("AWAY :Gone to lunch, back in 30 minutes"));

    // Should round-trip
    let parsed_away: Message = away_serialized.parse().expect("AWAY should parse");
    let parsed_away_msg: Message = away_msg_serialized
        .parse()
        .expect("AWAY with message should parse");

    assert_eq!(away_simple, parsed_away);
    assert_eq!(away_with_msg, parsed_away_msg);
}
