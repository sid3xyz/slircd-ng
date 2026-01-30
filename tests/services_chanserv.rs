mod common;
use common::TestServer;
use slirc_proto::Command;

#[tokio::test]
async fn test_chanserv_register_flow() -> anyhow::Result<()> {
    let server = TestServer::spawn(16669).await?; // Use non-default port

    // 1. User Connects
    let mut client = server.connect("Alice").await?;
    client.register().await?;

    // 2. Register Account via NickServ
    client
        .send(Command::PRIVMSG(
            "NickServ".to_string(),
            "REGISTER password123 alice@example.com".to_string(),
        ))
        .await?;

    // Wait for "registered" reply
    let _ = client
        .recv_until(|m| {
            m.command.to_string().contains("NOTICE") && m.to_string().contains("registered")
        })
        .await?;

    // 3. Join Channel
    client
        .send(Command::JOIN("#test".to_string(), None, None))
        .await?;
    // Wait for join
    let _ = client
        .recv_until(|m| m.command == Command::JOIN("#test".to_string(), None, None))
        .await?;

    // 4. Register Channel via ChanServ
    // ChanServ REGISTER #test This is a test channel
    client
        .send(Command::PRIVMSG(
            "ChanServ".to_string(),
            "REGISTER #test One amazing channel".to_string(),
        ))
        .await?;

    // 5. Verify Success
    let msgs = client
        .recv_until(|m| {
            m.command.to_string().contains("NOTICE")
                && m.to_string().contains("has been registered")
        })
        .await?;

    assert!(msgs.iter().any(|m| {
        m.to_string()
            .contains("Channel \x02#test\x02 has been registered")
    }));

    // 6. Verify Info
    client
        .send(Command::PRIVMSG(
            "ChanServ".to_string(),
            "INFO #test".to_string(),
        ))
        .await?;
    let info = client
        .recv_until(|m| m.to_string().contains("End of info"))
        .await?;

    assert!(
        info.iter()
            .any(|m| m.to_string().contains("Founder    : Alice"))
    );
    assert!(
        info.iter()
            .any(|m| m.to_string().contains("Description: One amazing channel"))
    );

    Ok(())
}
