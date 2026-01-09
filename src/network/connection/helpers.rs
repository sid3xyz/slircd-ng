use slirc_proto::{Command, Message, Prefix, Response};
use std::net::SocketAddr;

/// Build a flood warning notice.
pub fn flood_warning_notice(server_name: &str, violations: u8, max: u8) -> Message {
    Message::from(Command::NOTICE(
        "*".to_string(),
        format!(
            "*** Warning: Flooding detected ({}/{} strikes). Slow down or you will be disconnected.",
            violations, max
        ),
    ))
    .with_prefix(Prefix::ServerName(server_name.to_string()))
}

/// Build an ERROR message for excess flood.
pub fn excess_flood_error() -> Message {
    Message::from(Command::ERROR("Excess Flood (Strike limit reached)".into()))
}

/// Build a QUIT closing link message.
pub fn closing_link_error(addr: &SocketAddr, quit_msg: Option<&str>) -> Message {
    let text = match quit_msg {
        Some(msg) => format!("Closing Link: {} (Quit: {})", addr.ip(), msg),
        None => format!("Closing Link: {} (Client Quit)", addr.ip()),
    };
    Message {
        tags: None,
        prefix: None,
        command: Command::ERROR(text),
    }
}

/// Build an input too long error response.
pub fn input_too_long_response(server_name: &str, nick: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::Response(
            Response::ERR_INPUTTOOLONG,
            vec![nick.to_string(), "Input line too long".to_string()],
        ),
    }
}

/// Build a BATCH start message for labeled-response.
pub fn batch_start_msg(server_name: &str, batch_ref: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::BATCH(
            format!("+{}", batch_ref),
            Some(slirc_proto::BatchSubCommand::CUSTOM(
                "labeled-response".to_string(),
            )),
            None,
        ),
    }
}

/// Build a BATCH end message.
pub fn batch_end_msg(server_name: &str, batch_ref: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::BATCH(format!("-{}", batch_ref), None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::IrcEncode;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345)
    }

    fn to_string(msg: &Message) -> String {
        String::from_utf8_lossy(&msg.to_bytes()).to_string()
    }

    // ========================================================================
    // flood_warning_notice tests
    // ========================================================================

    #[test]
    fn flood_warning_notice_formats_correctly() {
        let msg = flood_warning_notice("irc.example.net", 2, 3);
        let encoded = to_string(&msg);
        assert!(encoded.contains(":irc.example.net NOTICE * :"));
        assert!(encoded.contains("2/3 strikes"));
        assert!(encoded.contains("Flooding detected"));
    }

    #[test]
    fn flood_warning_notice_shows_max_strikes() {
        let msg = flood_warning_notice("test.server", 5, 5);
        let encoded = to_string(&msg);
        assert!(encoded.contains("5/5 strikes"));
    }

    // ========================================================================
    // excess_flood_error tests
    // ========================================================================

    #[test]
    fn excess_flood_error_formats_correctly() {
        let msg = excess_flood_error();
        let encoded = to_string(&msg);
        assert!(encoded.starts_with("ERROR :"));
        assert!(encoded.contains("Excess Flood"));
        assert!(encoded.contains("Strike limit"));
    }

    // ========================================================================
    // closing_link_error tests
    // ========================================================================

    #[test]
    fn closing_link_with_quit_message() {
        let msg = closing_link_error(&test_addr(), Some("Leaving"));
        let encoded = to_string(&msg);
        assert!(encoded.starts_with("ERROR :Closing Link:"));
        assert!(encoded.contains("192.168.1.100"));
        assert!(encoded.contains("Quit: Leaving"));
    }

    #[test]
    fn closing_link_without_quit_message() {
        let msg = closing_link_error(&test_addr(), None);
        let encoded = to_string(&msg);
        assert!(encoded.starts_with("ERROR :Closing Link:"));
        assert!(encoded.contains("192.168.1.100"));
        assert!(encoded.contains("Client Quit"));
    }

    // ========================================================================
    // input_too_long_response tests
    // ========================================================================

    #[test]
    fn input_too_long_response_formats_correctly() {
        let msg = input_too_long_response("irc.example.net", "testnick");
        let encoded = to_string(&msg);
        assert!(encoded.contains(":irc.example.net"));
        assert!(encoded.contains("417")); // ERR_INPUTTOOLONG
        assert!(encoded.contains("testnick"));
        assert!(encoded.contains("Input line too long"));
    }

    // ========================================================================
    // batch_start_msg tests
    // ========================================================================

    #[test]
    fn batch_start_msg_formats_correctly() {
        let msg = batch_start_msg("irc.example.net", "abc123");
        let encoded = to_string(&msg);
        assert!(encoded.contains(":irc.example.net BATCH +abc123"));
        assert!(encoded.contains("labeled-response"));
    }

    // ========================================================================
    // batch_end_msg tests
    // ========================================================================

    #[test]
    fn batch_end_msg_formats_correctly() {
        let msg = batch_end_msg("irc.example.net", "abc123");
        let encoded = to_string(&msg);
        assert!(encoded.contains(":irc.example.net BATCH -abc123"));
    }

    #[test]
    fn batch_messages_use_matching_refs() {
        let start = batch_start_msg("srv", "ref123");
        let end = batch_end_msg("srv", "ref123");

        let start_enc = to_string(&start);
        let end_enc = to_string(&end);

        // Both should reference the same batch ID
        assert!(start_enc.contains("+ref123"));
        assert!(end_enc.contains("-ref123"));
    }
}
