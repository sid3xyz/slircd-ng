use slirc_proto::command::CommandRef;
use slirc_proto::compliance::{check_compliance, ComplianceConfig, ComplianceError};
use slirc_proto::message::MessageRef;
use smallvec::SmallVec;

#[test]
fn test_invalid_parameters() {
    let command = CommandRef::new("PRIVMSG", SmallVec::from(vec!["#channel", "Hello\nWorld"]));
    let msg = MessageRef {
        tags: None,
        prefix: None,
        command,
        raw: "PRIVMSG #channel :Hello\nWorld",
    };
    let config = ComplianceConfig::default();
    let result = check_compliance(&msg, None, &config);
    assert!(
        matches!(result, Err(errors) if matches!(errors[0], ComplianceError::InvalidParameter(_)))
    );
}

#[test]
fn test_valid_messages() {
    let raw = ":nick!user@host PRIVMSG #channel :Hello world!";
    let msg = MessageRef::parse(raw).unwrap();
    let config = ComplianceConfig::default();
    assert!(check_compliance(&msg, Some(raw.len()), &config).is_ok());

    let raw = "JOIN #channel";
    let msg = MessageRef::parse(raw).unwrap();
    assert!(check_compliance(&msg, Some(raw.len()), &config).is_ok());
}

#[test]
fn test_line_too_long() {
    let raw = format!(":nick!user@host PRIVMSG #channel :{}", "a".repeat(500));
    // Length will be > 512
    let msg = MessageRef::parse(&raw).unwrap();
    let config = ComplianceConfig::default();
    let result = check_compliance(&msg, Some(raw.len()), &config);
    assert!(matches!(result, Err(errors) if matches!(errors[0], ComplianceError::LineTooLong(_))));
}

#[test]
fn test_missing_parameters() {
    let raw = "PRIVMSG #channel";
    let msg = MessageRef::parse(raw).unwrap();
    let config = ComplianceConfig::default();
    let result = check_compliance(&msg, Some(raw.len()), &config);
    assert!(
        matches!(result, Err(errors) if matches!(errors[0], ComplianceError::MissingParameter("text")))
    );

    let raw = "JOIN";
    let msg = MessageRef::parse(raw).unwrap();
    let result = check_compliance(&msg, Some(raw.len()), &config);
    assert!(
        matches!(result, Err(errors) if matches!(errors[0], ComplianceError::MissingParameter("channel")))
    );
}

#[test]
fn test_strict_channel_names() {
    let raw = "JOIN invalid_channel";
    let msg = MessageRef::parse(raw).unwrap();
    let config = ComplianceConfig {
        strict_channel_names: true,
        ..Default::default()
    };

    let result = check_compliance(&msg, Some(raw.len()), &config);
    assert!(
        matches!(result, Err(errors) if matches!(errors[0], ComplianceError::InvalidChannelName(_)))
    );

    let raw = "JOIN #valid_channel";
    let msg = MessageRef::parse(raw).unwrap();
    assert!(check_compliance(&msg, Some(raw.len()), &config).is_ok());
}

#[test]
fn test_strict_nicknames() {
    let raw = "NICK 123invalid"; // Starts with digit
    let msg = MessageRef::parse(raw).unwrap();
    let config = ComplianceConfig {
        strict_nicknames: true,
        ..Default::default()
    };

    let result = check_compliance(&msg, Some(raw.len()), &config);
    assert!(
        matches!(result, Err(errors) if matches!(errors[0], ComplianceError::InvalidNickname(_)))
    );

    let raw = "NICK valid_nic";
    let msg = MessageRef::parse(raw).unwrap();
    assert!(check_compliance(&msg, Some(raw.len()), &config).is_ok());
}
