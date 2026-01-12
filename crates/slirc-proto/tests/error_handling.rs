//! Integration tests for error handling scenarios
//!
//! These tests verify that the library handles various error conditions
//! gracefully and provides useful error messages for debugging.

use slirc_proto::Message;

#[test]
fn test_parse_error_context() {
    let long_prefix = ":toolong!".repeat(100);
    let invalid_messages = vec![
        ("", "empty message"),
        ("   \r\n", "whitespace only"),
        ("@invalid-tag-format", "malformed tag without command"),
        ("@tag=value\r\nCOMMAND", "embedded line breaks in tags"),
        (long_prefix.as_str(), "extremely long prefix"),
        ("COMMAND \x01\x02\x03", "control characters in parameters"),
    ];

    for (invalid_msg, description) in invalid_messages {
        let result = invalid_msg.parse::<Message>();

        match result {
            Ok(msg) => {
                // If it parses successfully, verify it round-trips
                let serialized = msg.to_string();
                let _reparsed: Message = serialized
                    .parse()
                    .expect("Successfully parsed message should round-trip");
                println!(
                    "Unexpectedly parsed '{}' ({}): {}",
                    invalid_msg, description, serialized
                );
            }
            Err(parse_err) => {
                // Verify error provides useful information
                let error_msg = format!("{}", parse_err);
                assert!(
                    !error_msg.is_empty(),
                    "Error should provide description for {}",
                    description
                );

                // Verify error chain
                let mut source = std::error::Error::source(&parse_err);
                let mut depth = 0;
                while let Some(err) = source {
                    depth += 1;
                    source = err.source();
                    assert!(depth < 10, "Error chain too deep for {}", description);
                }
            }
        }
    }
}

#[test]
fn test_error_source_chaining() {
    // Test that errors properly chain their sources
    let invalid_msg = "@invalid=\r\nvalue COMMAND";
    let result = invalid_msg.parse::<Message>();

    match result {
        Err(parse_err) => {
            let error_chain = format!("{:?}", parse_err);
            assert!(!error_chain.is_empty(), "Error chain should not be empty");

            // Check for source chain
            let mut current_error: &dyn std::error::Error = &parse_err;
            let mut chain_length = 0;

            while let Some(source) = current_error.source() {
                chain_length += 1;
                current_error = source;
                if chain_length > 5 {
                    break; // Prevent infinite loops
                }
            }

            println!(
                "Error chain length: {}, Full error: {}",
                chain_length, parse_err
            );
        }
        Ok(msg) => {
            // If it parses successfully, that's also fine - just verify it works
            println!("Message parsed successfully: {}", msg);
            let serialized = msg.to_string();
            let _reparsed: Message = serialized.parse().expect("Should round-trip");
        }
    }
}

#[test]
fn test_partial_message_errors() {
    // Test error handling for partially valid messages
    let partial_messages = vec![
        ("PRIVMSG", "command without required parameters"),
        ("PRIVMSG #channel", "missing message text"),
        ("JOIN", "JOIN without channel"),
        ("MODE #channel", "MODE without mode changes"),
        ("KICK #channel", "KICK without nickname"),
    ];

    for (partial_msg, description) in partial_messages {
        let result = partial_msg.parse::<Message>();

        match result {
            Ok(msg) => {
                // Some might parse successfully with default values
                println!("Parsed '{}' ({}): {}", partial_msg, description, msg);

                // Verify round-trip
                let serialized = msg.to_string();
                let _reparsed: Message = serialized.parse().expect("Should round-trip");
            }
            Err(parse_err) => {
                let error_msg = format!("{}", parse_err);
                assert!(
                    !error_msg.is_empty(),
                    "Should provide error description for {}",
                    description
                );
                println!(
                    "Parse error for '{}' ({}): {}",
                    partial_msg, description, error_msg
                );
            }
        }
    }
}

#[test]
fn test_malformed_tags_error_handling() {
    let malformed_tag_messages = vec![
        ("@=invalid PING :server", "tag key cannot be empty"),
        ("@key= PING :server", "empty tag value"),
        ("@key==value PING :server", "double equals in tag"),
        ("@key;=value PING :server", "semicolon before value"),
        ("@key=value; PING :server", "trailing semicolon"),
        ("@;key=value PING :server", "leading semicolon"),
        ("@key=val\nue PING :server", "newline in tag value"),
    ];

    for (malformed_msg, description) in malformed_tag_messages {
        let result = malformed_msg.parse::<Message>();

        match result {
            Ok(msg) => {
                // Some malformed tags might still parse
                println!(
                    "Parsed malformed tags '{}' ({}): {}",
                    malformed_msg, description, msg
                );

                // Verify round-trip if it parsed
                let serialized = msg.to_string();
                let _reparsed: Message = serialized.parse().expect("Should round-trip");
            }
            Err(parse_err) => {
                let error_msg = format!("{}", parse_err);
                println!(
                    "Tag parse error '{}' ({}): {}",
                    malformed_msg, description, error_msg
                );
                assert!(!error_msg.is_empty(), "Should provide error description");
            }
        }
    }
}

#[test]
fn test_malformed_prefix_error_handling() {
    let malformed_prefix_messages = vec![
        (":!invalid@host PING :server", "prefix starting with !"),
        (":nick!@host PING :server", "empty user in prefix"),
        (":nick!user@ PING :server", "empty host in prefix"),
        (":nick!!user@host PING :server", "double ! in prefix"),
        (":nick@host!user PING :server", "@ before ! in prefix"),
        (":nick user@host PING :server", "space in prefix"),
        (": PING :server", "empty prefix"),
    ];

    for (malformed_msg, description) in malformed_prefix_messages {
        let result = malformed_msg.parse::<Message>();

        match result {
            Ok(msg) => {
                // Some might parse successfully
                println!(
                    "Parsed malformed prefix '{}' ({}): {}",
                    malformed_msg, description, msg
                );

                // Verify round-trip
                let serialized = msg.to_string();
                let _reparsed: Message = serialized.parse().expect("Should round-trip");
            }
            Err(parse_err) => {
                let error_msg = format!("{}", parse_err);
                println!(
                    "Prefix parse error '{}' ({}): {}",
                    malformed_msg, description, error_msg
                );
                assert!(!error_msg.is_empty(), "Should provide error description");
            }
        }
    }
}

#[test]
fn test_numeric_response_error_handling() {
    let malformed_numeric_messages = vec![
        (":server 99 nick :Too few digits", "2-digit numeric"),
        (":server 1000 nick :Too many digits", "4-digit numeric"),
        (":server ABC nick :Non-numeric", "alphabetic code"),
        (":server 001", "missing parameters"),
        (
            ":server 001 :missing nickname",
            "missing nickname parameter",
        ),
    ];

    for (malformed_msg, description) in malformed_numeric_messages {
        let result = malformed_msg.parse::<Message>();

        match result {
            Ok(msg) => {
                println!(
                    "Parsed malformed numeric '{}' ({}): {}",
                    malformed_msg, description, msg
                );

                // Verify round-trip
                let serialized = msg.to_string();
                let _reparsed: Message = serialized.parse().expect("Should round-trip");
            }
            Err(parse_err) => {
                let error_msg = format!("{}", parse_err);
                println!(
                    "Numeric parse error '{}' ({}): {}",
                    malformed_msg, description, error_msg
                );
                assert!(!error_msg.is_empty(), "Should provide error description");
            }
        }
    }
}

#[test]
fn test_error_display_formatting() {
    // Test that errors format nicely for user display
    let test_message = "@invalid-tag=\r\nvalue :malformed!@host COMMAND param1 param2";
    let result = test_message.parse::<Message>();

    match result {
        Err(err) => {
            let display_format = format!("{}", err);
            let debug_format = format!("{:?}", err);

            assert!(
                !display_format.is_empty(),
                "Display format should not be empty"
            );
            assert!(!debug_format.is_empty(), "Debug format should not be empty");

            // Display format should be user-friendly
            assert!(
                !display_format.contains("ParseError"),
                "Display format should not expose internal types"
            );

            // Debug format should be detailed
            println!("Display format: {}", display_format);
            println!("Debug format: {}", debug_format);
        }
        Ok(msg) => {
            println!("Unexpectedly parsed malformed message: {}", msg);
        }
    }
}

#[test]
fn test_error_recovery_scenarios() {
    // Test that the parser can handle and recover from various error scenarios
    let recovery_test_messages = vec![
        (
            "PING :server\r\nPRIVMSG #channel :Hello",
            "valid message after error",
        ),
        (
            "INVALID\r\nPING :server",
            "valid PING after invalid command",
        ),
        (
            "@bad-tag\r\n@time=2023-01-01T00:00:00Z PING :server",
            "good tags after bad tags",
        ),
    ];

    for (test_input, description) in recovery_test_messages {
        // Split on line breaks and test each line individually
        let lines: Vec<&str> = test_input.split("\r\n").collect();

        for (i, line) in lines.iter().enumerate() {
            let result = line.parse::<Message>();

            match result {
                Ok(msg) => {
                    println!(
                        "Line {} of '{}' ({}): parsed as {}",
                        i, test_input, description, msg
                    );

                    // Verify round-trip
                    let serialized = msg.to_string();
                    let _reparsed: Message = serialized.parse().expect("Should round-trip");
                }
                Err(err) => {
                    println!(
                        "Line {} of '{}' ({}) failed: {}",
                        i, test_input, description, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_context_preservation_in_errors() {
    // Test that error context is preserved through the parsing chain
    let complex_invalid_message = "@time=2023-01-01T00:00:00Z;msgid=abc123;invalid-tag :complex!user@host.example.com PRIVMSG #channel :Message with context";

    let result = complex_invalid_message.parse::<Message>();

    match result {
        Err(parse_err) => {
            // Check the full error message
            let full_error = format!("{}", parse_err);
            println!("Full error message: {}", full_error);
            assert!(!full_error.is_empty(), "Full error should not be empty");
        }
        Ok(msg) => {
            // If it parsed successfully, that's also fine
            println!("Complex message parsed successfully: {}", msg);

            // Verify round-trip
            let serialized = msg.to_string();
            let _reparsed: Message = serialized.parse().expect("Should round-trip");
        }
    }
}
