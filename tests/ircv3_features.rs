use slirc_proto::{BatchSubCommand, Command, Message, Tag};

#[test]
fn test_batch_command_serialization() {
    let cmd = Command::BATCH(
        "+batch1".to_string(),
        Some(BatchSubCommand::NETSPLIT),
        Some(vec!["server.name".to_string()]),
    );
    let msg = Message::from(cmd);
    assert_eq!(msg.to_string(), "BATCH +batch1 NETSPLIT server.name\r\n");

    let cmd_end = Command::BATCH("-batch1".to_string(), None, None);
    let msg_end = Message::from(cmd_end);
    assert_eq!(msg_end.to_string(), "BATCH -batch1\r\n");
}

#[test]
fn test_labeled_response_tag() {
    let mut msg = Message::from(Command::ACK);
    msg.tags = Some(vec![Tag::new("label", Some("12345".to_string()))]);

    assert_eq!(msg.to_string(), "@label=12345 ACK\r\n");
}

#[test]
fn test_message_tags_propagation() {
    let mut msg = Message::privmsg("#test", "Hello");
    msg.tags = Some(vec![
        Tag::new("label", Some("123".to_string())),
        Tag::new("+typing", None),
        Tag::new("time", Some("2023-01-01T00:00:00.000Z".to_string())),
    ]);

    let serialized = msg.to_string();
    assert!(serialized.contains("label=123"));
    assert!(serialized.contains("+typing"));
    assert!(serialized.contains("time=2023-01-01T00:00:00.000Z"));
}
