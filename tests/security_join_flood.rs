use crate::common::TestServer;
use slirc_proto::Command;

mod common;

#[tokio::test]
async fn test_join_flood_limit() -> anyhow::Result<()> {
    // Start a server on port 20001
    let server = TestServer::spawn(20001).await?;
    let mut client = server.connect("flooder").await?;
    client.register().await?;

    // Create 20 channels: #0, #1, ..., #19
    let mut channels = Vec::new();
    for i in 0..20 {
        channels.push(format!("#{}", i));
    }
    let channels_str = channels.join(",");

    // Send JOIN command with 20 targets and NO keys (keys=None)
    client.send(Command::JOIN(channels_str.clone(), None, None)).await?;

    // Wait and check
    let mut joined_count = 0;
    let mut received_error = false;

    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        match client.recv_timeout(std::time::Duration::from_millis(100)).await {
            Ok(msg) => {
                // println!("Received: {:?}", msg.command);
                if matches!(msg.command, Command::JOIN(_, _, _)) {
                    joined_count += 1;
                } else if let Command::Response(resp, _) = &msg.command {
                    if resp.code() == 407 {
                        received_error = true;
                    }
                }
            }
            Err(_) => {}
        }
        if joined_count >= 20 { break; }
    }

    assert!(joined_count <= 10, "Limit broken (no keys)");
    assert!(received_error, "No error received (no keys)");

    // Test case 2: 20 channels, 1 key (Panic reproduction attempt)
    // We need a new client/server or reset state? We can reuse server.
    // Client is likely in "joined" state for first 10 channels.
    // Let's use a new client.

    let mut client2 = server.connect("flooder2").await?;
    client2.register().await?;

    // Send JOIN with 20 channels and 1 key
    // Slirc-proto Command::JOIN takes Option<String> for key.
    // "key1" implies 1 key.
    client2.send(Command::JOIN(channels_str, Some("key1".to_string()), None)).await?;

    let mut joined_count2 = 0;
    let mut received_error2 = false;

    let start2 = std::time::Instant::now();
    while start2.elapsed() < std::time::Duration::from_secs(5) {
        match client2.recv_timeout(std::time::Duration::from_millis(100)).await {
            Ok(msg) => {
                if matches!(msg.command, Command::JOIN(_, _, _)) {
                    joined_count2 += 1;
                } else if let Command::Response(resp, _) = &msg.command {
                    if resp.code() == 407 {
                        received_error2 = true;
                    }
                }
            }
            Err(_) => {
                // If server crashed, recv_timeout might return Err or EndOfStream (which is Err in TestClient)
                // TestClient returns anyhow::Error on EOF.
            }
        }
        if joined_count2 >= 20 { break; }
    }

    // If server panicked, we wouldn't get here or recv would fail.
    // Assert limit is enforced.
    assert!(joined_count2 <= 10, "Limit broken (with keys)");
    assert!(received_error2, "No error received (with keys)");

    Ok(())
}
