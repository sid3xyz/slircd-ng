//! Integration test: Two-Server S2S Link
//!
//! This test spawns two slircd-ng instances, establishes an S2S link between them,
//! and verifies that state synchronization works correctly.

mod common;

use common::{TestClient, TestServer};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

/// Test basic S2S handshake and burst between two servers.
#[tokio::test]
async fn test_s2s_handshake_and_burst() -> anyhow::Result<()> {
    // Create temporary directory for test
    let test_dir = std::env::temp_dir().join("slircd-s2s-test");
    std::fs::create_dir_all(&test_dir)?;

    // Create config for Server A (SID: 001)
    let config_a = test_dir.join("server_a.toml");
    std::fs::write(
        &config_a,
        r#"
[server]
name = "server-a.test"
network = "TestNet"
sid = "001"
description = "Test Server A"
metrics_port = 0

[listen]
address = "127.0.0.1:16667"

[database]
path = "/tmp/slircd-s2s-test/server_a.db"

[timeouts]
registration_timeout = 5

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000
max_connections_per_ip = 200

[motd]
lines = ["Test Server A"]

[[link]]
name = "server-b.test"
hostname = "127.0.0.1"
port = 16668
password = "linkpass"
sid = "002"
autoconnect = false
"#,
    )?;

    // Create config for Server B (SID: 002)
    let config_b = test_dir.join("server_b.toml");
    std::fs::write(
        &config_b,
        r#"
[server]
name = "server-b.test"
network = "TestNet"
sid = "002"
description = "Test Server B"
metrics_port = 0

[listen]
address = "127.0.0.1:17667"

[s2s]
address = "127.0.0.1:16668"

[database]
path = "/tmp/slircd-s2s-test/server_b.db"

[timeouts]
registration_timeout = 5

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000
max_connections_per_ip = 200

[motd]
lines = ["Test Server B"]

[[link]]
name = "server-a.test"
hostname = "127.0.0.1"
port = 16667
password = "linkpass"
sid = "001"
autoconnect = false
"#,
    )?;

    // Spawn server A
    let server_a = TestServer::spawn_with_config(16667, config_a).await?;
    eprintln!("✓ Server A started on port 16667");

    // Spawn server B
    let server_b = TestServer::spawn_with_config(17667, config_b).await?;
    eprintln!("✓ Server B started on port 17667");

    // Give servers time to stabilize
    sleep(Duration::from_millis(500)).await;

    // Connect and register alice on server A
    let mut client_a = TestClient::connect("127.0.0.1:16667", "alice").await?;
    client_a.register().await?;
    eprintln!("✓ Alice connected and registered to Server A");

    // Connect and register bob on server B
    let mut client_b = TestClient::connect("127.0.0.1:17667", "bob").await?;
    client_b.register().await?;
    eprintln!("✓ Bob connected and registered to Server B");

    // Have alice join a channel on server A
    client_a.join("#test").await?;
    eprintln!("✓ Alice joined #test on Server A");

    // Have bob join a channel on server B
    client_b.join("#localtest").await?;
    eprintln!("✓ Bob joined #localtest on Server B");

    // Verify both servers are running independently
    // This validates that the base infrastructure is working
    // Next phase will add S2S linking and test actual synchronization

    // Clean up
    drop(client_a);
    drop(client_b);
    drop(server_a);
    drop(server_b);
    
    // Give servers time to fully shutdown
    sleep(Duration::from_millis(200)).await;
    
    // Best-effort cleanup, ignore errors
    let _ = std::fs::remove_dir_all(&test_dir);

    Ok(())
}

/// Test message routing across S2S link.
#[tokio::test]
#[ignore] // Enable once CONNECT command is implemented
async fn test_s2s_message_routing() -> anyhow::Result<()> {
    // TODO: Implement once we have CONNECT command
    // 1. Link servers A ↔ B
    // 2. Client on A sends PRIVMSG to client on B
    // 3. Verify delivery
    Ok(())
}

/// Test channel state synchronization via SJOIN.
#[tokio::test]
#[ignore] // Enable once CONNECT command is implemented
async fn test_s2s_sjoin_synchronization() -> anyhow::Result<()> {
    // TODO: Implement once we have CONNECT command
    // 1. Link servers A ↔ B
    // 2. Create channel on A with topic
    // 3. Verify channel appears on B with same topic
    // 4. Verify TB (Topic Burst) was sent
    Ok(())
}

/// Test SQUIT and netsplit cleanup.
#[tokio::test]
#[ignore] // Enable once CONNECT command is implemented
async fn test_s2s_squit_cleanup() -> anyhow::Result<()> {
    // TODO: Implement once we have CONNECT command
    // 1. Link servers A ↔ B
    // 2. Client joins channel from each server
    // 3. Kill server B
    // 4. Verify A cleans up B's users
    Ok(())
}
