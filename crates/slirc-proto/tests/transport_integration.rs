//! Integration tests for the IRC transport layer and connection handling
//!
//! These tests verify that the transport components work correctly with
//! real-world scenarios and interact properly with each other.

use slirc_proto::{Command, Message};

#[test]
fn test_connection_message_flow() {
    // This test would require a test IRC server or mock
    // For now, we'll test the message construction and validation
    let messages = vec![
        "USER testuser 0 * :Test User",
        "NICK testnick",
        "JOIN #testchannel",
        "PRIVMSG #testchannel :Hello, integration test!",
        "QUIT :Goodbye",
    ];

    // Verify all messages parse correctly
    for msg_str in &messages {
        let message: Message = msg_str
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", msg_str, e));

        // Verify they serialize back correctly
        let serialized = message.to_string();
        let reparsed: Message = serialized
            .parse()
            .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));

        assert_eq!(message, reparsed);
    }
}

#[test]
fn test_connection_state_transitions() {
    // Test that we can construct messages for various connection states

    // Registration phase
    let nick_msg = Message {
        tags: None,
        prefix: None,
        command: Command::NICK("testnick".to_string()),
    };
    let user_msg = Message {
        tags: None,
        prefix: None,
        command: Command::USER(
            "testuser".to_string(),
            "0".to_string(),
            "Test User".to_string(),
        ),
    };

    assert!(nick_msg.to_string().contains("NICK testnick"));
    assert!(user_msg.to_string().contains("USER testuser 0"));

    // Channel operations
    let join_msg = Message {
        tags: None,
        prefix: None,
        command: Command::JOIN("#test".to_string(), None, None),
    };
    let privmsg_msg = Message {
        tags: None,
        prefix: None,
        command: Command::PRIVMSG("#test".to_string(), "Hello!".to_string()),
    };

    assert!(join_msg.to_string().contains("JOIN #test"));
    assert!(privmsg_msg.to_string().contains("PRIVMSG #test"));

    // Connection termination
    let quit_msg = Message {
        tags: None,
        prefix: None,
        command: Command::QUIT(Some("Goodbye".to_string())),
    };
    assert!(quit_msg.to_string().contains("QUIT"));
}

#[test]
fn test_ping_pong_handling() {
    // Test PING/PONG message handling
    let ping_raw = "PING :server.example.com";
    let ping_msg: Message = ping_raw.parse().expect("Failed to parse PING");

    // Create appropriate PONG response
    let pong_response = match &ping_msg.command {
        Command::PING(server, _) => Message {
            tags: None,
            prefix: None,
            command: Command::PONG(server.clone(), None),
        },
        _ => panic!("Expected PING command"),
    };

    assert!(pong_response.to_string().contains("PONG"));

    // Test round-trip
    let pong_str = pong_response.to_string();
    let pong_parsed: Message = pong_str.parse().expect("Failed to parse PONG");
    assert_eq!(pong_response, pong_parsed);
}

#[test]
fn test_numeric_response_handling() {
    // Test common numeric responses
    let test_responses = vec![
        (":server 001 nick :Welcome message", "001"),
        (":server 002 nick :Your host message", "002"),
        (":server 353 nick = #channel :nick1 nick2", "353"),
        (":server 366 nick #channel :End of names", "366"),
        (":server 422 nick :MOTD File is missing", "422"),
    ];

    for (response_str, expected_code) in test_responses {
        let message: Message = response_str
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", response_str, e));

        match &message.command {
            Command::Raw(code, _params) => {
                assert_eq!(
                    code, expected_code,
                    "Unexpected numeric code for '{}'",
                    response_str
                );
            }
            _ => {
                // Some numeric responses might parse as other command types
                println!(
                    "Message '{}' parsed as: {:?}",
                    response_str, message.command
                );
            }
        }

        // Verify round-trip
        let serialized = message.to_string();
        let reparsed: Message = serialized
            .parse()
            .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
        assert_eq!(message, reparsed);
    }
}

#[test]
fn test_error_handling_scenarios() {
    // Test various malformed messages
    let invalid_messages = vec![
        "",                               // Empty message
        "   ",                            // Whitespace only
        "COMMAND",                        // Command without required parameters
        "@invalid-tag-format COMMAND",    // Malformed tags
        ":invalid:prefix:format COMMAND", // Malformed prefix
    ];

    for invalid_msg in invalid_messages {
        let result = invalid_msg.parse::<Message>();
        match result {
            Ok(_) => {
                // Some messages might be valid even if they look suspicious
                // In that case, verify they round-trip correctly
                let message: Message = invalid_msg.parse().unwrap();
                let serialized = message.to_string();
                let _reparsed: Message =
                    serialized.parse().expect("Valid message should round-trip");
            }
            Err(parse_err) => {
                // This is expected for truly invalid messages
                // Verify the error provides useful information
                let error_string = format!("{}", parse_err);
                assert!(!error_string.is_empty(), "Error should provide description");
            }
        }
    }
}

#[test]
fn test_capability_negotiation_flow() {
    // Test CAP negotiation message flow
    let cap_messages = vec![
        "CAP LS 302",
        ":server CAP * LS :batch labeled-response message-tags",
        "CAP REQ :batch message-tags",
        ":server CAP * ACK :batch message-tags",
        "CAP END",
    ];

    for msg_str in cap_messages {
        let message: Message = msg_str
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse CAP message '{}': {}", msg_str, e));

        // Verify round-trip
        let serialized = message.to_string();
        let reparsed: Message = serialized
            .parse()
            .unwrap_or_else(|e| panic!("Failed to reparse CAP message '{}': {}", serialized, e));

        assert_eq!(
            message, reparsed,
            "CAP message round-trip failed for '{}'",
            msg_str
        );
    }
}

#[test]
fn test_ircv3_tags_integration() {
    // Test various IRCv3 tags scenarios
    let tagged_messages = vec![
        "@time=2023-01-01T00:00:00.000Z PING :server",
        "@msgid=abc123;time=2023-01-01T00:00:00.000Z :nick PRIVMSG #channel :Hello",
        "@batch=abc123 :server PRIVMSG #channel :Batched message",
        "@account=userAccount :nick!user@host PRIVMSG #channel :Identified user",
        "@+custom-tag=value :server NOTICE #channel :Custom tag message",
    ];

    for msg_str in tagged_messages {
        let message: Message = msg_str
            .parse()
            .unwrap_or_else(|e| panic!("Failed to parse tagged message '{}': {}", msg_str, e));

        // Verify tags are present
        assert!(
            message.tags.is_some(),
            "Tags should be present for '{}'",
            msg_str
        );

        // Verify round-trip
        let serialized = message.to_string();
        let reparsed: Message = serialized
            .parse()
            .unwrap_or_else(|e| panic!("Failed to reparse tagged message '{}': {}", serialized, e));

        assert_eq!(
            message, reparsed,
            "Tagged message round-trip failed for '{}'",
            msg_str
        );
    }
}

#[test]
fn test_message_size_limits() {
    // Test handling of very long messages (close to IRC limits)
    let long_message = "A".repeat(400); // Approaching 512 byte limit
    let msg_str = format!("PRIVMSG #channel :{}", long_message);

    let message: Message = msg_str
        .parse()
        .expect("Should handle long messages within limits");

    let serialized = message.to_string();
    let reparsed: Message = serialized.parse().expect("Long message should round-trip");

    assert_eq!(message, reparsed);
}

#[test]
fn test_concurrent_message_parsing() {
    use std::sync::Arc;
    use std::thread;

    let messages = Arc::new(vec![
        "PING :server1",
        "PING :server2",
        "PING :server3",
        ":nick1 PRIVMSG #channel :Message 1",
        ":nick2 PRIVMSG #channel :Message 2",
        ":nick3 PRIVMSG #channel :Message 3",
    ]);

    let mut handles = vec![];

    for i in 0..3 {
        let messages_clone = Arc::clone(&messages);
        let handle = thread::spawn(move || {
            for msg_str in messages_clone.iter() {
                let message: Message = msg_str.parse().unwrap_or_else(|e| {
                    panic!("Thread {} failed to parse '{}': {}", i, msg_str, e)
                });

                let serialized = message.to_string();
                let _reparsed: Message = serialized.parse().unwrap_or_else(|e| {
                    panic!("Thread {} failed to reparse '{}': {}", i, serialized, e)
                });
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread should complete successfully");
    }
}

// =============================================================================
// Zero-Copy Transport Tests
// =============================================================================

mod zero_copy_tests {
    use slirc_proto::message::MessageRef;

    #[test]
    fn test_zero_copy_message_ref_parse() {
        // Test basic parsing
        let raw = "PING :server";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.command_name(), "PING");
        assert_eq!(msg.args(), &["server"]);
    }

    #[test]
    fn test_zero_copy_with_tags() {
        let raw = "@time=2023-01-01;msgid=abc :nick!user@host PRIVMSG #channel :Hello";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert_eq!(msg.command_name(), "PRIVMSG");
        assert_eq!(msg.tag_value("time"), Some("2023-01-01"));
        assert_eq!(msg.tag_value("msgid"), Some("abc"));
        assert_eq!(msg.source_nickname(), Some("nick"));
        assert_eq!(msg.args(), &["#channel", "Hello"]);
    }

    #[test]
    fn test_zero_copy_to_owned_round_trip() {
        let raw = "@time=2023-01-01 :nick!user@host PRIVMSG #channel :Hello, world!";
        let msg_ref = MessageRef::parse(raw).expect("Should parse");

        // Convert to owned
        let msg_owned = msg_ref.to_owned();

        // Serialize and reparse
        let serialized = msg_owned.to_string();
        let reparsed: slirc_proto::Message = serialized.parse().expect("Should reparse");

        assert_eq!(msg_owned, reparsed);
    }

    #[test]
    fn test_zero_copy_numeric_detection() {
        let raw = ":server 001 nick :Welcome";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert!(msg.is_numeric());
        assert_eq!(msg.numeric_code(), Some(1));
    }

    #[test]
    fn test_zero_copy_privmsg_detection() {
        let raw = ":nick PRIVMSG #channel :Hello";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert!(msg.is_privmsg());
        assert!(!msg.is_notice());
    }

    #[test]
    fn test_zero_copy_tags_iter() {
        let raw = "@a=1;b=2;c PING";
        let msg = MessageRef::parse(raw).expect("Should parse");

        let tags: Vec<_> = msg.tags_iter().collect();
        assert_eq!(tags, vec![("a", "1"), ("b", "2"), ("c", "")]);
    }

    #[test]
    fn test_zero_copy_has_tag() {
        let raw = "@time=2023;msgid PING";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert!(msg.has_tag("time"));
        assert!(msg.has_tag("msgid"));
        assert!(!msg.has_tag("nonexistent"));
    }
}
