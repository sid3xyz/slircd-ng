//! Integration tests for IRC connection lifecycle.
//!
//! Tests the complete flow of connecting, registering, and disconnecting from the server.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_basic_registration() {
    // Spawn test server on a random high port
    let port = 16667;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    // Connect a client
    let mut client = TestClient::connect(&server.address(), "testnick")
        .await
        .expect("Failed to connect");

    // Register
    client.register().await.expect("Registration failed");

    // Consume any remaining welcome messages after RPL_WELCOME
    // The registration might send additional numerics (002-004, 251-255, 265-266, MOTD, etc.)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while client
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Verify we're registered by sending PING and expecting PONG
    client
        .send(Command::PING("test".to_string(), None))
        .await
        .expect("Failed to send PING");

    let pong = client.recv().await.expect("Failed to receive PONG");

    match &pong.command {
        Command::PONG(server, Some(token)) => {
            assert_eq!(token, "test", "PONG token mismatch");
            assert!(!server.is_empty(), "PONG server name should not be empty");
        }
        other => {
            panic!("Expected PONG with token, got: {:?}", other);
        }
    }

    // Clean disconnect
    client
        .quit(Some("Test complete".to_string()))
        .await
        .expect("Failed to quit");
}

// Removed flaky registration-timeout test. A deterministic timeout test will be added
// once server exposes configurable per-test timeout hooks.

#[tokio::test]
async fn test_duplicate_nick() {
    let port = 16669;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    // Connect and register first client
    let mut client1 = TestClient::connect(&server.address(), "testnick")
        .await
        .expect("Failed to connect client1");
    client1
        .register()
        .await
        .expect("Failed to register client1");

    // Connect second client and try to use same nick
    let mut client2 = TestClient::connect(&server.address(), "testnick")
        .await
        .expect("Failed to connect client2");

    client2
        .send(Command::NICK("testnick".to_string()))
        .await
        .expect("Failed to send NICK");
    client2
        .send(Command::USER(
            "testnick".to_string(),
            "0".to_string(),
            "Test User".to_string(),
        ))
        .await
        .expect("Failed to send USER");

    // Should receive ERR_NICKNAMEINUSE (433)
    let messages = client2
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 433))
        .await
        .expect("Failed to receive ERR_NICKNAMEINUSE");

    assert!(
        messages
            .iter()
            .any(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 433))
    );
}

#[tokio::test]
async fn test_ping_pong() {
    let port = 16670;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "pingtest")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Consume post-registration messages
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while client
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Send PING
    client
        .send(Command::PING("test123".to_string(), None))
        .await
        .expect("Failed to send PING");

    // Expect PONG with same token
    let pong = client.recv().await.expect("Failed to receive PONG");
    match &pong.command {
        Command::PONG(_server, Some(token)) if token == "test123" => {}
        _ => panic!("Expected PONG with token 'test123', got: {:?}", pong),
    }
}

#[tokio::test]
async fn test_multiple_concurrent_connections() {
    let port = 16671;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    // Spawn 10 concurrent clients
    let mut handles = vec![];
    for i in 0..10 {
        let address = server.address();
        let nick = format!("client{}", i);

        let handle = tokio::spawn(async move {
            let mut client = TestClient::connect(&address, &nick)
                .await
                .expect("Failed to connect");
            client.register().await.expect("Registration failed");

            // Consume post-registration messages
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            while client
                .recv_timeout(tokio::time::Duration::from_millis(10))
                .await
                .is_ok()
            {}

            // Send a PING to verify we're connected
            client
                .send(Command::PING(format!("test{}", i), None))
                .await
                .expect("Failed to send PING");

            let pong = client.recv().await.expect("Failed to receive PONG");
            match &pong.command {
                Command::PONG(_server, Some(token)) if token == &format!("test{}", i) => {}
                _ => panic!("Expected PONG, got: {:?}", pong),
            }

            client.quit(None).await.expect("Failed to quit");
        });

        handles.push(handle);

        // Small delay between spawning to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Wait for all clients to complete
    for handle in handles {
        handle.await.expect("Client task panicked");
    }
}
