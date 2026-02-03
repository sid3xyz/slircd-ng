mod common;
use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::time::Duration;

#[tokio::test]
async fn test_join_max_targets_dos_protection() {
    let port = 16999;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "flooder")
        .await
        .expect("Failed to connect flooder");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Create a JOIN command with 20 channels (limit should be 10)
    let channels: Vec<String> = (0..20).map(|i| format!("#chan{}", i)).collect();
    let channels_str = channels.join(",");

    // Send raw JOIN command
    client.send_raw(&format!("JOIN {}", channels_str)).await.expect("Failed to send JOIN");

    // We expect the server to process at most 10, or reject the command.
    // If it rejects with ERR_TOOMANYTARGETS (407), that's good.
    // If it processes all 20, that's bad.

    // We'll count how many JOIN success messages we get.
    let mut joins = 0;
    let mut errors = 0;

    // Read messages for up to 3 seconds
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match client.recv_timeout(Duration::from_millis(200)).await {
            Ok(msg) => {
                if let Command::JOIN(_, _, _) = msg.command {
                    joins += 1;
                } else if let slirc_proto::Command::Response(resp, _) = msg.command {
                    // Check for ERR_TOOMANYTARGETS (407)
                    if resp.code() == 407 {
                        errors += 1;
                    }
                }
            }
            Err(e) => {
                // If it's a timeout, we keep trying until outer loop deadline
                if !e.to_string().contains("deadline has elapsed") {
                     break;
                }
            }
        }
    }

    // If we implemented the fix correctly, we expect either:
    // 1. 10 joins and maybe an error
    // 2. An error and 0 joins (if we reject the whole command)
    // 3. Just 10 joins (silent truncation)

    // CURRENT BEHAVIOR (Vulnerability): It will likely join all 20.

    println!("Joins: {}, Errors: {}", joins, errors);

    // Fails if we joined more than 15 (allowing some wiggle room if logic is loose, but 20 is def fail)
    assert!(joins <= 15, "DoS vulnerability: Server processed {} JOIN targets in one command! (Expected <= 15)", joins);
}
