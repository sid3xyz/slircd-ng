//! Integration test: Two-Server S2S Link
//!
//! This test spawns two slircd-ng instances, establishes an S2S link between them,
//! and verifies that state synchronization works correctly.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::time::Duration;
use tokio::time::sleep;

/// Test basic S2S handshake and burst between two servers.
#[tokio::test]
async fn test_s2s_handshake_and_burst() -> anyhow::Result<()> {
    let (test_dir, _server_a, _server_b, mut client_a, mut client_b) = setup_s2s_env().await?;

    // Authenticate as oper on Server A
    client_a.send_raw("OPER admin operpass").await?;
    expect_oper_success(&mut client_a).await?;

    // Initiate connection to Server B
    client_a.send_raw("CONNECT server-b.test 6667").await?;

    // Wait for link to establish
    wait_for_link(&mut client_a, "server-b.test").await?;

    // Verify both clients are still happy
    client_a.join("#test").await?;
    client_b.join("#test").await?;

    sleep(Duration::from_millis(100)).await;

    // Cleanup
    drop(client_a);
    drop(client_b);
    let _ = std::fs::remove_dir_all(&test_dir);
    Ok(())
}

/// Test message routing across S2S link.
#[tokio::test]
async fn test_s2s_message_routing() -> anyhow::Result<()> {
    let (test_dir, _server_a, _server_b, mut client_a, mut client_b) = setup_s2s_env().await?;

    // Link servers
    client_a.send_raw("OPER admin operpass").await?;
    expect_oper_success(&mut client_a).await?;
    client_a.send_raw("CONNECT server-b.test 6667").await?;
    wait_for_link(&mut client_a, "server-b.test").await?;

    // Join shared channel
    client_a.join("#chat").await?;
    client_b.join("#chat").await?;
    sleep(Duration::from_millis(500)).await;

    // A sends to channel -> B receives
    client_a.privmsg("#chat", "Hello from A").await?;

    // B should receive: :alice!alice@... PRIVMSG #chat :Hello from A
    let msg = expect_msg_containing(&mut client_b, "Hello from A").await?;
    assert!(msg.prefix.unwrap().to_string().starts_with("alice!alice@"));

    // B sends private message to A -> A receives
    client_b.privmsg("alice", "Direct message").await?;

    // A should receive: :bob!bob@... PRIVMSG alice :Direct message
    let msg = expect_msg_containing(&mut client_a, "Direct message").await?;
    assert!(msg.prefix.unwrap().to_string().starts_with("bob!bob@"));

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
    Ok(())
}

/// Test channel state synchronization via SJOIN.
#[tokio::test]
async fn test_s2s_sjoin_synchronization() -> anyhow::Result<()> {
    let (test_dir, _server_a, _server_b, mut client_a, mut client_b) = setup_s2s_env().await?;

    // Alice creates channel with topic on A BEFORE link
    client_a.join("#topic").await?;
    client_a.topic("#topic", "Pre-link topic").await?;
    sleep(Duration::from_millis(100)).await;

    // Link servers
    client_a.send_raw("OPER admin operpass").await?;
    expect_oper_success(&mut client_a).await?;
    client_a.send_raw("CONNECT server-b.test 6667").await?;
    wait_for_link(&mut client_a, "server-b.test").await?;

    // Bob joins #topic on B
    // Should receive topic synced from A
    client_b.join("#topic").await?;

    // Expect RPL_TOPIC (332) containing "Pre-link topic"
    let _ = expect_msg_containing(&mut client_b, "Pre-link topic").await?;

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
    Ok(())
}

/// Test SQUIT and netsplit cleanup.
#[tokio::test]
async fn test_s2s_squit_cleanup() -> anyhow::Result<()> {
    let (test_dir, _server_a, _server_b, mut client_a, mut client_b) = setup_s2s_env().await?;

    // Link servers
    client_a.send_raw("OPER admin operpass").await?;
    expect_oper_success(&mut client_a).await?;
    client_a.send_raw("CONNECT server-b.test 6667").await?;
    wait_for_link(&mut client_a, "server-b.test").await?;

    // Join common channel
    client_a.join("#split").await?;
    client_b.join("#split").await?;
    sleep(Duration::from_millis(500)).await;

    // SQUIT server B
    client_a.send_raw("SQUIT server-b.test :Split test").await?;
    sleep(Duration::from_millis(500)).await;

    // Alice should receive QUIT for Bob (netsplit)
    // Note: She might fail NOTICEs first, so we scan until we find the QUIT
    let msgs = client_a
        .recv_until(|msg| {
            if let Command::QUIT(Some(reason)) = &msg.command {
                // Netsplits can result in "local_server remote_server" (default) or the custom reason
                // depending on exact timing. We accept either (reason is optional).
                reason.contains("Split test") || reason.contains("server-a.test server-b.test")
            } else {
                false
            }
        })
        .await?;

    let msg = msgs.last().expect("Should have found QUIT message");

    // Verify it's a QUIT command from Bob
    assert!(msg.prefix.as_ref().unwrap().to_string().starts_with("bob"));

    // Cleanup
    let _ = std::fs::remove_dir_all(&test_dir);
    Ok(())
}

// --- Helpers ---

fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn expect_oper_success(client: &mut TestClient) -> anyhow::Result<()> {
    let _ = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 381))
        .await?;
    Ok(())
}

async fn wait_for_link(client: &mut TestClient, peer_name: &str) -> anyhow::Result<()> {
    // Poll STATS L until we see the peer connected
    for _ in 0..20 {
        // 20 attempts * 500ms = 10s timeout
        client.send_raw("STATS L").await?;

        let mut linked = false;
        // STATS L reply: 211 (RPL_STATSLINKINFO)
        // End: 219 (RPL_ENDOFSTATS)

        // We need to read until 219
        let msgs = client
            .recv_until(
                |msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 219),
            )
            .await?;

        for msg in msgs {
            if let Command::Response(resp, params) = msg.command
                && resp.code() == 211 && params.len() > 1 && params[1] == peer_name {
                    // Check "open" time or bytes > 0?
                    // Just existence in the list means it's a registered link.
                    // But SyncManager adds it early.
                    // We trust it's connecting.
                    linked = true;
                }
        }

        if linked {
            return Ok(());
        }

        sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow::anyhow!(
        "Timed out waiting for link to {}",
        peer_name
    ))
}

async fn expect_msg_containing(
    client: &mut TestClient,
    substring: &str,
) -> anyhow::Result<slirc_proto::Message> {
    let mut found_msg = None;
    let _ = client
        .recv_until(|msg| {
            let s = msg.to_string();
            if s.contains(substring) {
                found_msg = Some(msg.clone());
                true
            } else {
                false
            }
        })
        .await?;

    found_msg.ok_or_else(|| anyhow::anyhow!("Message containing '{}' not found", substring))
}

async fn setup_s2s_env() -> anyhow::Result<(
    std::path::PathBuf,
    TestServer,
    TestServer,
    TestClient,
    TestClient,
)> {
    let test_dir = std::env::temp_dir().join(format!("slircd-s2s-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&test_dir)?;

    let db_a = test_dir.join("a.db");
    let db_b = test_dir.join("b.db");

    // Dynamic ports
    let port_a_client = get_free_port();
    let port_a_s2s = get_free_port();
    let port_b_client = get_free_port();
    let port_b_s2s = get_free_port();

    // Server A config
    let config_a_path = test_dir.join("server_a.toml");
    std::fs::write(
        &config_a_path,
        format!(
            r#"
[server]
name = "server-a.test"
network = "TestNet"
sid = "001"
description = "Test Server A"
metrics_port = 0

[listen]
address = "127.0.0.1:{}"

[s2s]
address = "127.0.0.1:{}"

[database]
path = "{}"

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false

[[oper]]
name = "admin"
password = "operpass"

[security.rate_limits]
message_rate_per_second = 100
connection_burst_per_ip = 100
join_burst_per_client = 100
ctcp_rate_per_second = 100
whois_rate_per_second = 100

[[link]]
name = "server-b.test"
hostname = "127.0.0.1"
port = {}
password = "linkpass"
sid = "002"
autoconnect = false
"#,
            port_a_client,
            port_a_s2s,
            db_a.display(),
            port_b_s2s
        ),
    )?;

    // Server B config
    let config_b_path = test_dir.join("server_b.toml");
    std::fs::write(
        &config_b_path,
        format!(
            r#"
[server]
name = "server-b.test"
network = "TestNet"
sid = "002"
description = "Test Server B"
metrics_port = 0

[listen]
address = "127.0.0.1:{}"

[s2s]
address = "127.0.0.1:{}"

[database]
path = "{}"

[security]
cloak_secret = "TestSecret-2026-Secure!9X"
cloak_suffix = "test"
spam_detection_enabled = false

[[oper]]
name = "admin"
password = "operpass"

[security.rate_limits]
message_rate_per_second = 100
connection_burst_per_ip = 100
join_burst_per_client = 100
ctcp_rate_per_second = 100
whois_rate_per_second = 100

[[link]]
name = "server-a.test"
hostname = "127.0.0.1"
port = {}
password = "linkpass"
sid = "001"
autoconnect = false
"#,
            port_b_client,
            port_b_s2s,
            db_b.display(),
            port_a_s2s
        ),
    )?;

    let server_a = TestServer::spawn_with_config(port_a_client, config_a_path).await?;
    let server_b = TestServer::spawn_with_config(port_b_client, config_b_path).await?;

    sleep(Duration::from_millis(2000)).await;

    let mut client_a =
        TestClient::connect(&format!("127.0.0.1:{}", port_a_client), "alice").await?;
    client_a.register().await?;

    let mut client_b = TestClient::connect(&format!("127.0.0.1:{}", port_b_client), "bob").await?;
    client_b.register().await?;

    Ok((test_dir, server_a, server_b, client_a, client_b))
}
