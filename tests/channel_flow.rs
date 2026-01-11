//! Integration tests for channel flows: JOIN and PRIVMSG.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_channel_privmsg_flow() {
    let port = 16672;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    // Connect two clients
    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect alice");
    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("Failed to connect bob");

    // Register both
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

    // Alice sends a message
    alice
        .privmsg("#test", "hello from alice")
        .await
        .expect("Alice privmsg failed");

    // Bob should eventually receive a PRIVMSG to #test
    // Read until we see the PRIVMSG
    let messages = bob
        .recv_until(|msg| matches!(&msg.command, Command::PRIVMSG(target, text) if target == "#test" && text.contains("hello")))
        .await
        .expect("Bob failed to receive PRIVMSG");

    assert!(messages.iter().any(|m| match &m.command {
        Command::PRIVMSG(target, text) => target == "#test" && text.contains("hello"),
        _ => false,
    }));

    // Cleanly disconnect both clients
    alice.quit(Some("done".to_string())).await.expect("Alice quit failed");
    bob.quit(Some("done".to_string())).await.expect("Bob quit failed");
}
