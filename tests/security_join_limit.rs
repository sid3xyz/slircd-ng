mod common;
use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_join_max_targets() {
    let port = 19001;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");

    client.register().await.expect("Alice registration failed");

    // Drain welcome
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while client.recv_timeout(tokio::time::Duration::from_millis(10)).await.is_ok() {}

    // Construct a JOIN with 11 channels
    let channels: Vec<String> = (1..=11).map(|i| format!("#chan{}", i)).collect();
    let join_cmd = format!("JOIN {}", channels.join(","));

    client.send_raw(&join_cmd).await.expect("Failed to send JOIN");

    // Currently (before fix), we expect 11 JOIN responses
    let mut joins = 0;
    let start = std::time::Instant::now();

    // Read messages for up to 5 seconds
    let mut received_error = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        match client.recv_timeout(tokio::time::Duration::from_millis(200)).await {
            Ok(msg) => {
                match &msg.command {
                    Command::JOIN(chan, _, _) => {
                        if chan.starts_with("#chan") {
                            joins += 1;
                        }
                    }
                    Command::Response(resp, _) if resp.code() == 407 => {
                        received_error = true;
                    }
                    _ => {}
                }
            }
            Err(_e) => {}
        }
        if joins >= 11 { break; }
    }

    assert_eq!(joins, 0, "Should have joined 0 channels (request rejected)");
    assert!(received_error, "Should have received ERR_TOOMANYTARGETS (407)");
}
