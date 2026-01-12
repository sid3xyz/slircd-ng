//! Comprehensive RFC 1459/2812 and IRCv3 compliance tests.
//!
//! This module tests specific edge cases and requirements from:
//! - RFC 1459: Internet Relay Chat Protocol
//! - RFC 2812: Internet Relay Chat: Client Protocol
//! - IRCv3 Message Tags: https://ircv3.net/specs/extensions/message-tags
//!
//! Run with: `cargo test --test rfc_ircv3_compliance`

use slirc_proto::{Command, Message, MessageRef};

// Note: Tag escaping tests have been moved to src/message/tags.rs

// =============================================================================
// IRCv3 TAG PARSING IN MESSAGES
// =============================================================================

mod tag_parsing {
    use super::*;

    #[test]
    fn test_tag_with_escaped_semicolon() {
        let raw = "@key=value\\:with\\:semicolons :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        let owned = msg.to_owned();

        // The tag value should have actual semicolons after unescaping
        let value = owned.tag_value("key");
        assert_eq!(value, Some("value;with;semicolons"));
    }

    #[test]
    fn test_tag_with_escaped_spaces() {
        let raw = "@key=hello\\sworld :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        let owned = msg.to_owned();

        assert_eq!(owned.tag_value("key"), Some("hello world"));
    }

    #[test]
    fn test_tag_without_value() {
        // IRCv3 allows tags without values (flag-style)
        let raw = "@+typing :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert!(msg.has_tag("+typing"));
        // Value should be empty string for flag tags
        assert_eq!(msg.tag_value("+typing"), Some(""));
    }

    #[test]
    fn test_multiple_tags_mixed() {
        let raw = "@+typing;time=2023-01-01T00:00:00Z;msgid=abc :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");

        assert!(msg.has_tag("+typing"));
        assert_eq!(msg.tag_value("time"), Some("2023-01-01T00:00:00Z"));
        assert_eq!(msg.tag_value("msgid"), Some("abc"));
    }

    #[test]
    fn test_client_only_tag_prefix() {
        // Client-only tags start with +
        let raw = "@+example.com/custom=value :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.tag_value("+example.com/custom"), Some("value"));
    }

    #[test]
    fn test_vendor_prefixed_tag() {
        // Vendor-prefixed tags
        let raw = "@example.com/foo=bar :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.tag_value("example.com/foo"), Some("bar"));
    }
}

// =============================================================================
// RFC 1459/2812 MESSAGE FORMAT
// =============================================================================

mod message_format {
    use super::*;

    #[test]
    fn test_max_line_length_512() {
        // RFC 1459/2812: Maximum message length is 512 bytes including CRLF
        let long_text = "a".repeat(500);
        let raw = format!("PRIVMSG #ch :{}\r\n", long_text);

        // Should parse (but compliance check would flag it)
        let msg: Message = raw.parse().expect("Should parse");
        match &msg.command {
            Command::PRIVMSG(_, text) => assert_eq!(text.len(), 500),
            _ => panic!("Expected PRIVMSG"),
        }
    }

    #[test]
    fn test_crlf_line_ending() {
        let raw = "PING :server\r\n";
        let msg = MessageRef::parse(raw).expect("Should parse with CRLF");
        assert_eq!(msg.command_name(), "PING");
    }

    #[test]
    fn test_lf_only_line_ending() {
        // Many servers accept LF-only
        let raw = "PING :server\n";
        let msg = MessageRef::parse(raw).expect("Should parse with LF only");
        assert_eq!(msg.command_name(), "PING");
    }

    #[test]
    fn test_no_line_ending() {
        // Parser should handle messages without line ending
        let raw = "PING :server";
        let msg = MessageRef::parse(raw).expect("Should parse without line ending");
        assert_eq!(msg.command_name(), "PING");
    }

    #[test]
    fn test_empty_trailing_parameter() {
        // Empty trailing is valid: "PRIVMSG #ch :" means empty message
        let raw = "PRIVMSG #channel :";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.args(), &["#channel", ""]);
    }

    #[test]
    fn test_trailing_with_spaces() {
        let raw = ":nick PRIVMSG #ch :hello world with spaces";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.arg(1), Some("hello world with spaces"));
    }

    #[test]
    fn test_trailing_preserves_leading_colon() {
        // Double colon at start of trailing: the second colon is literal
        let raw = "PRIVMSG #ch ::starts with colon";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.arg(1), Some(":starts with colon"));
    }

    #[test]
    fn test_numeric_command() {
        // Numeric responses are 3 digits
        let raw = ":server 001 nick :Welcome to the network";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert!(msg.is_numeric());
        assert_eq!(msg.numeric_code(), Some(1));
    }

    #[test]
    fn test_max_params_15() {
        // RFC allows up to 15 parameters (14 middle + 1 trailing)
        let raw = "CMD 1 2 3 4 5 6 7 8 9 10 11 12 13 14 :15th trailing";
        let msg = MessageRef::parse(raw).expect("Should parse 15 params");
        assert_eq!(msg.args().len(), 15);
        assert_eq!(msg.arg(14), Some("15th trailing"));
    }
}

// =============================================================================
// PREFIX PARSING (RFC 2812 Section 2.3.1)
// =============================================================================

mod prefix_parsing {
    use super::*;

    #[test]
    fn test_full_user_prefix() {
        // nick!user@host format
        let raw = ":nick!user@host.example.com PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.source_nickname(), Some("nick"));
        assert_eq!(msg.source_user(), Some("user"));
        assert_eq!(msg.source_host(), Some("host.example.com"));
    }

    #[test]
    fn test_nick_at_host_prefix() {
        // Some servers send nick@host (no user)
        let raw = ":nick@host.example.com PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.source_nickname(), Some("nick"));
        // User may or may not be present depending on parser behavior
    }

    #[test]
    fn test_nick_only_prefix() {
        // Just nickname
        let raw = ":nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.source_nickname(), Some("nick"));
    }

    #[test]
    fn test_server_prefix() {
        // Server names contain dots
        let raw = ":irc.example.com 001 nick :Welcome";
        let msg = MessageRef::parse(raw).expect("Should parse");
        // Server prefix should be detected
        assert!(msg.prefix.is_some());
    }

    #[test]
    fn test_ipv6_host() {
        // IPv6 in host
        let raw = ":nick!user@2001:db8::1 PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse IPv6 host");
        assert_eq!(msg.source_nickname(), Some("nick"));
    }

    #[test]
    fn test_cloaked_host() {
        // Cloaked/masked hosts
        let raw = ":nick!user@user/nick/cloaked PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse cloaked host");
        assert_eq!(msg.source_host(), Some("user/nick/cloaked"));
    }
}

// =============================================================================
// CHANNEL NAMES (RFC 2812 Section 1.3)
// =============================================================================

mod channel_names {
    use super::*;

    #[test]
    fn test_standard_channel() {
        let raw = "JOIN #channel";
        let msg: Message = raw.parse().expect("Should parse");
        match msg.command {
            Command::JOIN(ch, _, _) => assert_eq!(ch, "#channel"),
            _ => panic!("Expected JOIN"),
        }
    }

    #[test]
    fn test_local_channel() {
        // & prefix is local channel
        let raw = "JOIN &localchan";
        let msg: Message = raw.parse().expect("Should parse");
        match msg.command {
            Command::JOIN(ch, _, _) => assert_eq!(ch, "&localchan"),
            _ => panic!("Expected JOIN"),
        }
    }

    #[test]
    fn test_channel_with_special_chars() {
        // Channels can contain special characters (except space, bell, comma)
        let raw = "JOIN #foo-bar_baz";
        let msg: Message = raw.parse().expect("Should parse");
        match msg.command {
            Command::JOIN(ch, _, _) => assert_eq!(ch, "#foo-bar_baz"),
            _ => panic!("Expected JOIN"),
        }
    }

    #[test]
    fn test_multiple_channels_join() {
        let raw = "JOIN #chan1,#chan2,#chan3";
        let msg: Message = raw.parse().expect("Should parse");
        match msg.command {
            Command::JOIN(ch, _, _) => assert_eq!(ch, "#chan1,#chan2,#chan3"),
            _ => panic!("Expected JOIN"),
        }
    }
}

// =============================================================================
// UTF-8 HANDLING (IRCv3 implies UTF-8)
// =============================================================================

mod utf8_handling {
    use super::*;

    #[test]
    fn test_utf8_in_message() {
        let raw = ":nick PRIVMSG #ch :Hello 世界 🌍";
        let msg = MessageRef::parse(raw).expect("Should parse UTF-8");
        assert_eq!(msg.arg(1), Some("Hello 世界 🌍"));
    }

    #[test]
    fn test_utf8_in_nick() {
        // Some servers allow UTF-8 nicks
        let raw = ":Ñoño!user@host PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse UTF-8 nick");
        assert_eq!(msg.source_nickname(), Some("Ñoño"));
    }

    #[test]
    fn test_utf8_in_tag_value() {
        let raw = "@label=föö :nick PRIVMSG #ch :hi";
        let msg = MessageRef::parse(raw).expect("Should parse UTF-8 in tag");
        assert_eq!(msg.tag_value("label"), Some("föö"));
    }

    #[test]
    fn test_emoji_in_message() {
        let raw = ":nick PRIVMSG #ch :🎉🎊🎈";
        let msg = MessageRef::parse(raw).expect("Should parse emoji");
        assert_eq!(msg.arg(1), Some("🎉🎊🎈"));
    }
}

// =============================================================================
// ROUND-TRIP COMPLIANCE
// =============================================================================

mod roundtrip {
    use super::*;

    fn assert_roundtrip(raw: &str) {
        let msg: Message = raw.parse().expect("Should parse");
        let serialized = msg.to_string();
        let reparsed: Message = serialized.parse().expect("Should reparse");
        assert_eq!(msg, reparsed, "Roundtrip failed for: {}", raw);
    }

    #[test]
    fn test_roundtrip_simple() {
        assert_roundtrip("PING :server");
    }

    #[test]
    fn test_roundtrip_with_prefix() {
        assert_roundtrip(":nick!user@host PRIVMSG #channel :Hello world");
    }

    #[test]
    fn test_roundtrip_with_tags() {
        assert_roundtrip("@time=2023-01-01T00:00:00Z;msgid=abc :nick PRIVMSG #ch :Tagged");
    }

    #[test]
    fn test_roundtrip_empty_trailing() {
        assert_roundtrip("PRIVMSG #channel :");
    }

    #[test]
    fn test_roundtrip_numeric() {
        assert_roundtrip(":server 001 nick :Welcome to the network");
    }

    #[test]
    fn test_roundtrip_with_escaped_tags() {
        // This tests that tags with special characters survive roundtrip
        let original = Message {
            tags: Some(vec![slirc_proto::Tag::new(
                "key",
                Some("value;with;semicolons".to_string()),
            )]),
            prefix: None,
            command: Command::PING("test".to_string(), None),
        };

        let serialized = original.to_string();
        let reparsed: Message = serialized.parse().expect("Should reparse");
        assert_eq!(original, reparsed);
        assert_eq!(reparsed.tag_value("key"), Some("value;with;semicolons"));
    }
}

// =============================================================================
// COMMAND-SPECIFIC TESTS
// =============================================================================

mod commands {
    use super::*;

    #[test]
    fn test_privmsg_requires_target_and_text() {
        let msg: Message = "PRIVMSG #channel :Hello".parse().unwrap();
        match msg.command {
            Command::PRIVMSG(target, text) => {
                assert_eq!(target, "#channel");
                assert_eq!(text, "Hello");
            }
            _ => panic!("Expected PRIVMSG"),
        }
    }

    #[test]
    fn test_notice_similar_to_privmsg() {
        let msg: Message = "NOTICE #channel :Hello".parse().unwrap();
        match msg.command {
            Command::NOTICE(target, text) => {
                assert_eq!(target, "#channel");
                assert_eq!(text, "Hello");
            }
            _ => panic!("Expected NOTICE"),
        }
    }

    #[test]
    fn test_join_with_key() {
        let msg: Message = "JOIN #channel secretkey".parse().unwrap();
        match msg.command {
            Command::JOIN(chan, key, _) => {
                assert_eq!(chan, "#channel");
                assert_eq!(key, Some("secretkey".to_string()));
            }
            _ => panic!("Expected JOIN"),
        }
    }

    #[test]
    fn test_join_variations_roundtrip() {
        let test_cases = vec![
            "JOIN #channel",
            "JOIN #channel key",
            ":nick!user@host JOIN #channel",
            "JOIN #channel1,#channel2 key1,key2",
        ];

        for original in test_cases {
            let message: Message = original
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", original, e));
            let serialized = message.to_string();
            let reparsed: Message = serialized
                .parse()
                .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
            assert_eq!(message, reparsed, "Round-trip failed for '{}'", original);
        }
    }

    #[test]
    fn test_part_with_message() {
        let msg: Message = "PART #channel :Goodbye!".parse().unwrap();
        match msg.command {
            Command::PART(chan, reason) => {
                assert_eq!(chan, "#channel");
                assert_eq!(reason, Some("Goodbye!".to_string()));
            }
            _ => panic!("Expected PART"),
        }
    }

    #[test]
    fn test_quit_with_message() {
        let msg: Message = "QUIT :Gone fishing".parse().unwrap();
        match msg.command {
            Command::QUIT(reason) => {
                assert_eq!(reason, Some("Gone fishing".to_string()));
            }
            _ => panic!("Expected QUIT"),
        }
    }

    #[test]
    fn test_mode_channel() {
        let msg: Message = "MODE #channel +o nick".parse().unwrap();
        // Verify it parses - channel MODE uses ChannelMODE variant
        assert!(matches!(msg.command, Command::ChannelMODE(_, _)));
    }

    #[test]
    fn test_mode_roundtrip() {
        let original = ":server MODE #channel +o nick";
        let message: Message = original.parse().expect("Failed to parse message");
        let serialized = message.to_string();
        let reparsed: Message = serialized.parse().expect("Failed to reparse message");
        assert_eq!(message, reparsed);
    }

    #[test]
    fn test_kick_with_reason() {
        let msg: Message = "KICK #channel nick :Bad behavior".parse().unwrap();
        match msg.command {
            Command::KICK(chan, target, reason) => {
                assert_eq!(chan, "#channel");
                assert_eq!(target, "nick");
                assert_eq!(reason, Some("Bad behavior".to_string()));
            }
            _ => panic!("Expected KICK"),
        }
    }

    #[test]
    fn test_batch_messages() {
        let test_cases = vec![
            "BATCH +abc123 chathistory #channel",
            "BATCH -abc123",
            "@batch=abc123 :server PRIVMSG #channel :Batched message",
        ];

        for original in test_cases {
            let message: Message = original
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", original, e));
            let serialized = message.to_string();
            let reparsed: Message = serialized
                .parse()
                .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
            assert_eq!(message, reparsed, "Round-trip failed for '{}'", original);
        }
    }
}

// =============================================================================
// WEBIRC COMMAND (WebSocket Gateway Support)
// =============================================================================

mod webirc {
    use super::*;

    #[test]
    fn test_webirc_basic() {
        // Standard WEBIRC format: WEBIRC password gateway hostname ip
        let raw = "WEBIRC secret_password TheLounge client.example.com 192.168.1.100";
        let msg: Message = raw.parse().expect("Should parse WEBIRC");

        match msg.command {
            Command::WEBIRC(password, gateway, hostname, ip, options) => {
                assert_eq!(password, "secret_password");
                assert_eq!(gateway, "TheLounge");
                assert_eq!(hostname, "client.example.com");
                assert_eq!(ip, "192.168.1.100");
                assert!(options.is_none());
            }
            other => panic!("Expected WEBIRC, got {:?}", other),
        }
    }

    #[test]
    fn test_webirc_with_options() {
        // WEBIRC with optional flags/options (trailing parameter)
        let raw = "WEBIRC password KiwiIRC user.host.org 10.0.0.50 :secure";
        let msg: Message = raw.parse().expect("Should parse WEBIRC with options");

        match msg.command {
            Command::WEBIRC(password, gateway, hostname, ip, options) => {
                assert_eq!(password, "password");
                assert_eq!(gateway, "KiwiIRC");
                assert_eq!(hostname, "user.host.org");
                assert_eq!(ip, "10.0.0.50");
                assert_eq!(options, Some("secure".to_string()));
            }
            other => panic!("Expected WEBIRC with options, got {:?}", other),
        }
    }

    #[test]
    fn test_webirc_ipv6() {
        // WEBIRC with IPv6 address
        let raw = "WEBIRC pass gateway hostname.com 2001:db8::1";
        let msg: Message = raw.parse().expect("Should parse WEBIRC with IPv6");

        match msg.command {
            Command::WEBIRC(_, _, _, ip, _) => {
                assert_eq!(ip, "2001:db8::1");
            }
            other => panic!("Expected WEBIRC, got {:?}", other),
        }
    }

    #[test]
    fn test_webirc_with_prefix() {
        // WEBIRC shouldn't normally have a prefix, but test it doesn't break
        let raw = ":gateway.server WEBIRC pass gateway host 1.2.3.4";
        let msg: Message = raw.parse().expect("Should parse WEBIRC with prefix");

        match msg.command {
            Command::WEBIRC(password, gateway, hostname, ip, _) => {
                assert_eq!(password, "pass");
                assert_eq!(gateway, "gateway");
                assert_eq!(hostname, "host");
                assert_eq!(ip, "1.2.3.4");
            }
            other => panic!("Expected WEBIRC, got {:?}", other),
        }
        assert!(msg.prefix.is_some());
    }

    #[test]
    fn test_webirc_roundtrip() {
        let test_cases = vec![
            "WEBIRC secret_password TheLounge client.example.com 192.168.1.100",
            "WEBIRC pass KiwiIRC host.org 10.0.0.1 :secure",
            "WEBIRC p gateway hostname 2001:db8:85a3::8a2e:370:7334",
        ];

        for original in test_cases {
            let message: Message = original
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", original, e));
            let serialized = message.to_string();
            let reparsed: Message = serialized
                .parse()
                .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
            assert_eq!(message, reparsed, "Round-trip failed for '{}'", original);
        }
    }

    #[test]
    fn test_webirc_insufficient_args_falls_back_to_raw() {
        // WEBIRC with less than 4 arguments should fall back to Raw
        let raw = "WEBIRC password gateway hostname";
        let msg: Message = raw.parse().expect("Should parse as Raw");

        match &msg.command {
            Command::Raw(cmd, args) => {
                assert_eq!(cmd, "WEBIRC");
                assert_eq!(args.len(), 3);
            }
            other => panic!("Expected Raw for insufficient WEBIRC args, got {:?}", other),
        }
    }

    #[test]
    fn test_webirc_common_gateways() {
        // Test parsing for common WebSocket IRC gateways
        let gateways = vec![
            (
                "WEBIRC pass TheLounge webclient.example.com 192.168.1.1",
                "TheLounge",
            ),
            ("WEBIRC pass KiwiIRC kiwi.host.net 10.0.0.100", "KiwiIRC"),
            (
                "WEBIRC pass IRCCloud irccloud-user.host 172.16.0.50",
                "IRCCloud",
            ),
            ("WEBIRC pass Mibbit mibbit.user.host 8.8.8.8", "Mibbit"),
            (
                "WEBIRC pass Glowing-Bear gb.user.host 1.1.1.1",
                "Glowing-Bear",
            ),
        ];

        for (raw, expected_gateway) in gateways {
            let msg: Message = raw.parse().unwrap_or_else(|e| {
                panic!("Failed to parse gateway '{}': {}", expected_gateway, e)
            });

            match msg.command {
                Command::WEBIRC(_, gateway, _, _, _) => {
                    assert_eq!(gateway, expected_gateway);
                }
                other => panic!("Expected WEBIRC for {}, got {:?}", expected_gateway, other),
            }
        }
    }

    #[test]
    fn test_webirc_serialization() {
        // Test that serialization produces valid IRC protocol output
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::WEBIRC(
                "password".to_string(),
                "Gateway".to_string(),
                "client.host".to_string(),
                "192.168.1.1".to_string(),
                None,
            ),
        };

        let serialized = msg.to_string();
        // to_string() includes trailing CRLF
        assert_eq!(
            serialized,
            "WEBIRC password Gateway client.host 192.168.1.1\r\n"
        );
    }

    #[test]
    fn test_webirc_serialization_with_options() {
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::WEBIRC(
                "password".to_string(),
                "Gateway".to_string(),
                "client.host".to_string(),
                "192.168.1.1".to_string(),
                Some("secure tls".to_string()),
            ),
        };

        let serialized = msg.to_string();
        // to_string() includes trailing CRLF
        assert_eq!(
            serialized,
            "WEBIRC password Gateway client.host 192.168.1.1 :secure tls\r\n"
        );
    }
}

// =============================================================================
// OPERATOR COMMANDS (KLINE, DLINE, KNOCK, etc.)
// =============================================================================

mod operator_commands {
    use super::*;

    #[test]
    fn test_kline_with_time() {
        let msg: Message = "KLINE 60 user@host :reason".parse().unwrap();
        match msg.command {
            Command::KLINE(Some(time), mask, reason) => {
                assert_eq!(time, "60");
                assert_eq!(mask, "user@host");
                assert_eq!(reason, "reason");
            }
            other => panic!("Expected KLINE with time, got {:?}", other),
        }
    }

    #[test]
    fn test_kline_without_time() {
        let msg: Message = "KLINE user@host :reason".parse().unwrap();
        match msg.command {
            Command::KLINE(None, mask, reason) => {
                assert_eq!(mask, "user@host");
                assert_eq!(reason, "reason");
            }
            other => panic!("Expected KLINE without time, got {:?}", other),
        }
    }

    #[test]
    fn test_dline_with_time() {
        let msg: Message = "DLINE 3600 192.168.0.1 :banned".parse().unwrap();
        match msg.command {
            Command::DLINE(Some(time), host, reason) => {
                assert_eq!(time, "3600");
                assert_eq!(host, "192.168.0.1");
                assert_eq!(reason, "banned");
            }
            other => panic!("Expected DLINE with time, got {:?}", other),
        }
    }

    #[test]
    fn test_unkline() {
        let msg: Message = "UNKLINE user@host".parse().unwrap();
        match msg.command {
            Command::UNKLINE(mask) => assert_eq!(mask, "user@host"),
            other => panic!("Expected UNKLINE, got {:?}", other),
        }
    }

    #[test]
    fn test_undline() {
        let msg: Message = "UNDLINE 10.0.0.0/8".parse().unwrap();
        match msg.command {
            Command::UNDLINE(host) => assert_eq!(host, "10.0.0.0/8"),
            other => panic!("Expected UNDLINE, got {:?}", other),
        }
    }

    #[test]
    fn test_knock_with_message() {
        let msg: Message = "KNOCK #channel :let me in".parse().unwrap();
        match msg.command {
            Command::KNOCK(channel, Some(message)) => {
                assert_eq!(channel, "#channel");
                assert_eq!(message, "let me in");
            }
            other => panic!("Expected KNOCK with message, got {:?}", other),
        }
    }

    #[test]
    fn test_knock_without_message() {
        let msg: Message = "KNOCK #channel".parse().unwrap();
        match msg.command {
            Command::KNOCK(channel, None) => {
                assert_eq!(channel, "#channel");
            }
            other => panic!("Expected KNOCK without message, got {:?}", other),
        }
    }

    #[test]
    fn test_operator_ban_commands_roundtrip() {
        let test_cases = vec![
            "KLINE 60 *@badhost.com :Spamming",
            "KLINE user@host.com :No reason given",
            "DLINE 3600 192.168.1.0/24 :Network abuse",
            "DLINE 10.0.0.1 :Suspicious activity",
            "UNKLINE user@host.com",
            "UNDLINE 192.168.1.0/24",
            "KNOCK #channel",
            "KNOCK #secretroom :Please let me in!",
        ];

        for original in test_cases {
            let message: Message = original
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", original, e));
            let serialized = message.to_string();
            let reparsed: Message = serialized
                .parse()
                .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
            assert_eq!(message, reparsed, "Round-trip failed for '{}'", original);
        }
    }
}

// =============================================================================
// EDGE CASES AND ERROR HANDLING
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_message_fails() {
        let result = MessageRef::parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_only_fails() {
        let result = MessageRef::parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_consecutive_spaces() {
        // Extra spaces between parts should be handled
        let raw = ":nick  PRIVMSG  #ch  :hello";
        // This might fail strict parsing but should not panic
        let _ = MessageRef::parse(raw);
    }

    #[test]
    fn test_very_long_nick() {
        // Extremely long nickname (non-compliant but shouldn't crash)
        let long_nick = "a".repeat(100);
        let raw = format!(":{}!user@host PRIVMSG #ch :hi", long_nick);
        let msg = MessageRef::parse(&raw).expect("Should handle long nick");
        assert_eq!(msg.source_nickname(), Some(long_nick.as_str()));
    }

    #[test]
    fn test_trailing_only_colon() {
        // Message with just a colon as trailing should work
        let raw = "PRIVMSG #ch ::";
        let msg = MessageRef::parse(raw).expect("Should parse");
        assert_eq!(msg.arg(1), Some(":"));
    }
}

// =============================================================================
// SERVICE COMMANDS AND ALIASES
// =============================================================================

mod service_commands {
    use super::*;

    #[test]
    fn test_nickserv_single_arg() {
        let msg: Message = "NICKSERV IDENTIFY".parse().unwrap();
        match msg.command {
            Command::NICKSERV(args) => {
                assert_eq!(args, vec!["IDENTIFY"]);
            }
            other => panic!("Expected NICKSERV, got {:?}", other),
        }
    }

    #[test]
    fn test_nickserv_multiple_args() {
        let msg: Message = "NICKSERV IDENTIFY mypassword".parse().unwrap();
        match msg.command {
            Command::NICKSERV(args) => {
                assert_eq!(args, vec!["IDENTIFY", "mypassword"]);
            }
            other => panic!("Expected NICKSERV, got {:?}", other),
        }
    }

    #[test]
    fn test_chanserv_multiple_args() {
        let msg: Message = "CHANSERV OP #channel nick".parse().unwrap();
        match msg.command {
            Command::CHANSERV(args) => {
                assert_eq!(args, vec!["OP", "#channel", "nick"]);
            }
            other => panic!("Expected CHANSERV, got {:?}", other),
        }
    }

    #[test]
    fn test_ns_alias() {
        let msg: Message = "NS IDENTIFY mypassword".parse().unwrap();
        match msg.command {
            Command::NS(args) => {
                assert_eq!(args, vec!["IDENTIFY", "mypassword"]);
            }
            other => panic!("Expected NS, got {:?}", other),
        }
    }

    #[test]
    fn test_cs_alias() {
        let msg: Message = "CS OP #channel nick".parse().unwrap();
        match msg.command {
            Command::CS(args) => {
                assert_eq!(args, vec!["OP", "#channel", "nick"]);
            }
            other => panic!("Expected CS, got {:?}", other),
        }
    }

    #[test]
    fn test_os_alias() {
        let msg: Message = "OS GLOBAL :Server maintenance at 3 PM".parse().unwrap();
        match msg.command {
            Command::OS(args) => {
                assert_eq!(args, vec!["GLOBAL", "Server maintenance at 3 PM"]);
            }
            other => panic!("Expected OS, got {:?}", other),
        }
    }

    #[test]
    fn test_ms_alias() {
        let msg: Message = "MS SEND nick :Hello there!".parse().unwrap();
        match msg.command {
            Command::MS(args) => {
                assert_eq!(args, vec!["SEND", "nick", "Hello there!"]);
            }
            other => panic!("Expected MS, got {:?}", other),
        }
    }

    #[test]
    fn test_hs_alias() {
        let msg: Message = "HS ON".parse().unwrap();
        match msg.command {
            Command::HS(args) => {
                assert_eq!(args, vec!["ON"]);
            }
            other => panic!("Expected HS, got {:?}", other),
        }
    }

    #[test]
    fn test_bs_alias() {
        let msg: Message = "BS ASSIGN #channel BotName".parse().unwrap();
        match msg.command {
            Command::BS(args) => {
                assert_eq!(args, vec!["ASSIGN", "#channel", "BotName"]);
            }
            other => panic!("Expected BS, got {:?}", other),
        }
    }

    #[test]
    fn test_service_commands_roundtrip() {
        let test_cases = vec![
            "NICKSERV IDENTIFY password",
            "CHANSERV OP #channel nick",
            "OPERSERV GLOBAL :Hello everyone",
            "BOTSERV ASSIGN #channel Bot",
            "HOSTSERV ON",
            "MEMOSERV SEND nick :Hello",
            "NS IDENTIFY password",
            "CS OP #channel nick",
            "OS GLOBAL :Hello everyone",
            "BS ASSIGN #channel Bot",
            "HS ON",
            "MS SEND nick :Hello",
        ];

        for original in test_cases {
            let message: Message = original
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", original, e));
            let serialized = message.to_string();
            let reparsed: Message = serialized
                .parse()
                .unwrap_or_else(|e| panic!("Failed to reparse '{}': {}", serialized, e));
            assert_eq!(message, reparsed, "Round-trip failed for '{}'", original);
        }
    }

    #[test]
    fn test_service_aliases_case_insensitive() {
        // IRC commands are case-insensitive
        let cases = vec![
            ("ns identify", "NS"),
            ("Ns identify", "NS"),
            ("NS IDENTIFY", "NS"),
            ("cs op #ch", "CS"),
            ("Cs op #ch", "CS"),
        ];

        for (input, expected_cmd) in cases {
            let msg: Message = input.parse().unwrap();
            let serialized = msg.to_string();
            assert!(
                serialized.starts_with(expected_cmd),
                "Expected '{}' to serialize starting with '{}', got '{}'",
                input,
                expected_cmd,
                serialized
            );
        }
    }
}
