use crate::common::{TestClient, TestServer};
use slirc_proto::Command;

mod common;

#[tokio::test]
async fn test_channel_key_security() {
    let port = 20000;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn server");

    let mut client1 = TestClient::connect(&server.address(), "admin")
        .await
        .expect("Failed to connect client1");
    client1
        .register()
        .await
        .expect("Failed to register client1");

    // Drain welcome
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while client1
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Client 1 creates channel and sets key
    client1
        .send(Command::JOIN("#secret".to_string(), None, None))
        .await
        .expect("Failed to join");

    // Wait for JOIN response
    let _ = client1
        .recv_until(|msg| matches!(msg.command, Command::JOIN(..)))
        .await;

    // Set key
    client1
        .send_raw("MODE #secret +k secret_password")
        .await
        .expect("Failed to set mode");

    // Wait for MODE response
    let _ = client1
        .recv_until(|msg| matches!(msg.command, Command::Response(..)))
        .await;

    let mut client2 = TestClient::connect(&server.address(), "user")
        .await
        .expect("Failed to connect client2");
    client2
        .register()
        .await
        .expect("Failed to register client2");

    // Drain welcome
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while client2
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Test 1: Join with no key (should fail with 475)
    client2
        .send(Command::JOIN("#secret".to_string(), None, None))
        .await
        .expect("Failed to send JOIN");

    let messages = client2
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 475))
        .await
        .expect("Should receive 475");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 475))
    );

    // Test 2: Join with wrong key (should fail with 475)
    client2
        .send(Command::JOIN(
            "#secret".to_string(),
            Some("wrong_password".to_string()),
            None,
        ))
        .await
        .expect("Failed to send JOIN");

    let messages = client2
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 475))
        .await
        .expect("Should receive 475");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 475))
    );

    // Test 3: Join with correct key (should succeed)
    client2
        .send(Command::JOIN(
            "#secret".to_string(),
            Some("secret_password".to_string()),
            None,
        ))
        .await
        .expect("Failed to send JOIN");

    let messages = client2
        .recv_until(|msg| matches!(&msg.command, Command::JOIN(..)))
        .await
        .expect("Should receive JOIN");

    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.command, Command::JOIN(..)))
    );
}
