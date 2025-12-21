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
