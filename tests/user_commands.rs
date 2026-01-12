//! Integration tests for user commands: AWAY, NICK, MODE, USERHOST, etc.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_away_command() {
    let port = 16680;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");

    alice.register().await.expect("Alice registration failed");

    // Drain welcome burst
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Set AWAY message
    alice
        .send(Command::AWAY(Some("Gone for lunch".to_string())))
        .await
        .expect("Failed to send AWAY");

    // Expect RPL_NOWAWAY (306)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 306))
        .await
        .expect("Failed to receive RPL_NOWAWAY");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 306))
    );

    // Unset AWAY
    alice
        .send(Command::AWAY(None))
        .await
        .expect("Failed to send AWAY");

    // Expect RPL_UNAWAY (305)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 305))
        .await
        .expect("Failed to receive RPL_UNAWAY");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 305))
    );

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_nick_change() {
    let port = 16681;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");

    alice.register().await.expect("Alice registration failed");

    // Drain welcome burst
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Change nick
    alice
        .send(Command::NICK("alice2".to_string()))
        .await
        .expect("Failed to send NICK");

    // Should receive NICK echo
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::NICK(new_nick) if new_nick == "alice2"))
        .await
        .expect("Failed to receive NICK echo");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::NICK(new_nick) if new_nick == "alice2"))
    );

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_nick_collision_with_channel() {
    let port = 16682;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");
    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("Failed to connect bob");

    alice.register().await.expect("Alice registration failed");
    bob.register().await.expect("Bob registration failed");

    // Drain welcome bursts
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}
    while bob
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Join channel
    alice.join("#test").await.expect("Alice join failed");
    bob.join("#test").await.expect("Bob join failed");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}
    while bob
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Alice changes nick - bob should see it
    alice
        .send(Command::NICK("alice_new".to_string()))
        .await
        .expect("Alice NICK failed");

    // Bob should receive NICK notification
    let messages = bob
        .recv_until(
            |msg| matches!(&msg.command, Command::NICK(new_nick) if new_nick == "alice_new"),
        )
        .await
        .expect("Bob failed to receive NICK notification");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::NICK(new_nick) if new_nick == "alice_new"))
    );

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_user_mode_changes() {
    let port = 16683;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");

    alice.register().await.expect("Alice registration failed");

    // Drain welcome burst
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Set +i (invisible) and check for response
    // Server may send MODE echo or just accept silently
    alice
        .send_raw("MODE alice +i")
        .await
        .expect("Failed to send MODE");

    // Give server time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Drain any responses
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Verify the MODE was accepted by querying it back
    alice
        .send_raw("MODE alice")
        .await
        .expect("Failed to query MODE");

    // Expect RPL_UMODEIS (221) response
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 221))
        .await
        .expect("Failed to receive MODE query response");

    // Check that response indicates +i is set
    let has_invisible = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 221 => {
            params.iter().any(|p| p.contains('i'))
        }
        _ => false,
    });

    assert!(has_invisible, "MODE query should show +i is set");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_userhost_command() {
    let port = 16684;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");

    alice.register().await.expect("Alice registration failed");

    // Drain welcome burst
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Send USERHOST command
    alice
        .send_raw("USERHOST alice")
        .await
        .expect("Failed to send USERHOST");

    // Expect RPL_USERHOST (302)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 302))
        .await
        .expect("Failed to receive RPL_USERHOST");

    let has_userhost = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 302 => {
            params.iter().any(|p| p.contains("alice"))
        }
        _ => false,
    });

    assert!(
        has_userhost,
        "USERHOST response should contain alice's info"
    );

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_quit_with_reason() {
    let port = 16685;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");
    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("Failed to connect bob");

    alice.register().await.expect("Alice registration failed");
    bob.register().await.expect("Bob registration failed");

    // Drain welcome bursts
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}
    while bob
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Join same channel
    alice.join("#quit").await.expect("Alice join failed");
    bob.join("#quit").await.expect("Bob join failed");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}
    while bob
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Alice quits with reason
    alice
        .quit(Some("Testing QUIT broadcast".to_string()))
        .await
        .expect("Alice quit failed");

    // Bob should receive QUIT notification
    let messages = bob
        .recv_until(
            |msg| matches!(&msg.command, Command::QUIT(Some(reason)) if reason.contains("Testing")),
        )
        .await
        .expect("Bob failed to receive QUIT notification");

    assert!(
        messages.iter().any(
            |m| matches!(&m.command, Command::QUIT(Some(reason)) if reason.contains("Testing"))
        )
    );

    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}
