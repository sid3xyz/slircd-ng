// tests/operator_commands.rs
//! Integration tests for operator commands: OPER, KILL, WALLOPS

mod common;
use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::time::Duration;

async fn drain(client: &mut TestClient) {
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}
}

#[tokio::test]
async fn test_oper_login_success() {
    let port = 16700;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");
    alice.register().await.expect("Registration failed");
    drain(&mut alice).await;

    // OPER using credentials from config.test.toml
    alice
        .send_raw("OPER testop testpass")
        .await
        .expect("Failed to send OPER");

    // Expect RPL_YOUREOPER (381) and user MODE +o echo
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 381))
        .await
        .expect("Expected YOU'RE OPER");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 381))
    );

    // Drain possible MODE +o echo
    drain(&mut alice).await;
}

#[tokio::test]
async fn test_oper_login_failure() {
    let port = 16701;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");
    alice.register().await.expect("Registration failed");
    drain(&mut alice).await;

    // Wrong password
    alice
        .send_raw("OPER testop wrongpass")
        .await
        .expect("Failed to send OPER");

    // Expect ERR_PASSWDMISMATCH (464) or ERR_NOOPERHOST (491) depending on path
    let _ = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 464 || resp.code() == 491))
        .await
        .expect("Expected oper failure numeric");
}

#[tokio::test]
async fn test_kill_command_disconnects_target() {
    let port = 16702;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect Alice");
    alice.register().await.expect("Registration failed");

    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("Failed to connect Bob");
    bob.register().await.expect("Registration failed");

    drain(&mut alice).await;
    drain(&mut bob).await;

    // Alice becomes oper
    alice
        .send_raw("OPER testop testpass")
        .await
        .expect("Failed to send OPER");
    let _ = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 381))
        .await
        .expect("Expected YOU'RE OPER");

    // Alice KILLs bob
    alice
        .send_raw("KILL bob :testing kill")
        .await
        .expect("Failed to send KILL");

    // Bob should receive ERROR then disconnect; observe any ERROR
    let messages = bob
        .recv_until(|msg| matches!(&msg.command, Command::ERROR(_)))
        .await
        .expect("Bob should receive ERROR before disconnect");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::ERROR(_)))
    );
}

#[tokio::test]
async fn test_wallops_broadcast_to_wallops_users() {
    let port = 16703;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect Alice");
    alice.register().await.expect("Registration failed");

    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("Failed to connect Bob");
    bob.register().await.expect("Registration failed");

    drain(&mut alice).await;
    drain(&mut bob).await;

    // Alice becomes oper
    alice
        .send_raw("OPER testop testpass")
        .await
        .expect("Failed to send OPER");
    let _ = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 381))
        .await
        .expect("Expected YOU'RE OPER");

    // Bob sets +w to receive wallops
    bob.send_raw("MODE bob +w").await.expect("Failed to set +w");
    drain(&mut bob).await;

    // Alice sends WALLOPS
    alice
        .send_raw("WALLOPS :system maintenance")
        .await
        .expect("Failed to send WALLOPS");

    // Alice should see WALLOPS echo, Bob should receive WALLOPS
    let alice_msgs = alice
        .recv_until(|msg| matches!(&msg.command, Command::WALLOPS(_)))
        .await
        .expect("Alice should see WALLOPS echo");
    assert!(alice_msgs.iter().any(
        |m| matches!(&m.command, Command::WALLOPS(text) if text.contains("system maintenance"))
    ));

    let bob_msgs = bob
        .recv_until(|msg| matches!(&msg.command, Command::WALLOPS(_)))
        .await
        .expect("Bob should receive WALLOPS");
    assert!(bob_msgs.iter().any(
        |m| matches!(&m.command, Command::WALLOPS(text) if text.contains("system maintenance"))
    ));
}
