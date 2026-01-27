use crate::common::TestServer;
use slirc_proto::{Command, CapSubCommand};
use tokio::time::sleep;
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_chathistory_before() -> anyhow::Result<()> {
    let port = 26680;
    let server = TestServer::spawn(port).await?;
    let mut client1 = server.connect("user1").await?;
    let mut client2 = server.connect("user2").await?;

    client1.register().await?;
    client2.register().await?;
    
    // Request capabilities
    client2.send(Command::CAP(
        None, 
        CapSubCommand::REQ, 
        Some("batch server-time msgid draft/chathistory".to_string()), 
        None
    )).await?;
    // Drain CAP ACK (simplified)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    while client2.recv_timeout(std::time::Duration::from_millis(10)).await.is_ok() {}

    client1.send(Command::JOIN("#history".to_string(), None, None)).await?;
    client2.send(Command::JOIN("#history".to_string(), None, None)).await?;
    
    // Create some history
    client1.privmsg("#history", "Message 1").await?;
    sleep(Duration::from_millis(200)).await;
    client1.privmsg("#history", "Message 2").await?;
    sleep(Duration::from_millis(200)).await;
    client1.privmsg("#history", "Message 3").await?;
    sleep(Duration::from_millis(200)).await;

    // 4. Request history (before now)
    client2.send_raw("CHATHISTORY BEFORE #history * 5").await?;

    // 5. Read until batch end
    // Expect BATCH start/end or messages?
    // Usually a batch starts with +reference, sends messages, then -reference.
    // We'll collect until we see BATCH end (starts with -) or timeout.
    // Note: The specific batch reference string is random.
    
    let messages = client2.recv_until(|msg| {
        if let Command::BATCH(ref_tag, _, _) = &msg.command {
            // End of batch usually has no other params, or just reference minus.
            // Actually slircd sends BATCH -tag
            ref_tag.starts_with('-')
        } else {
            false
        }
    }).await?;

    // Verify batch contents
    let history_msgs: Vec<_> = messages.iter()
        .filter_map(|m| {
            if let Command::PRIVMSG(_, text) = &m.command {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();
    
    println!("Received messages: {:?}", messages);
    assert!(history_msgs.len() >= 3, "Should receive at least the 3 messages sent. Got: {:?}", history_msgs);
    assert!(history_msgs.iter().any(|text| text == "Message 1"));
    assert!(history_msgs.iter().any(|text| text == "Message 2"));
    assert!(history_msgs.iter().any(|text| text == "Message 3"));

    Ok(())
}
