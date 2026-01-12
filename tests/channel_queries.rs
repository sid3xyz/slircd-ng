//! Integration tests for channel query commands: LIST, WHO, WHOWAS.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_list_command() {
    let port = 16690;
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

    // Create a couple of channels
    alice.join("#test1").await.expect("Failed to join #test1");
    alice.join("#test2").await.expect("Failed to join #test2");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Request channel LIST
    alice.send_raw("LIST").await.expect("Failed to send LIST");

    // Expect RPL_LISTSTART (321), RPL_LIST (322) entries, RPL_LISTEND (323)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 323))
        .await
        .expect("Failed to receive LIST response");

    // Should have RPL_LIST entries for our channels
    let has_test1 = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 322 => {
            params.iter().any(|p| p.contains("#test1"))
        }
        _ => false,
    });

    let has_test2 = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 322 => {
            params.iter().any(|p| p.contains("#test2"))
        }
        _ => false,
    });

    assert!(has_test1, "LIST should include #test1");
    assert!(has_test2, "LIST should include #test2");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_list_with_pattern() {
    let port = 16691;
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

    // Create channels with different patterns
    alice.join("#foo1").await.expect("Failed to join #foo1");
    alice.join("#bar2").await.expect("Failed to join #bar2");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Request LIST with pattern
    alice
        .send_raw("LIST #foo*")
        .await
        .expect("Failed to send LIST");

    // Expect RPL_LISTEND (323)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 323))
        .await
        .expect("Failed to receive LIST response");

    // Should have #foo1 but not #bar2
    let has_foo = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 322 => {
            params.iter().any(|p| p.contains("#foo"))
        }
        _ => false,
    });

    let has_bar = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 322 => {
            params.iter().any(|p| p.contains("#bar"))
        }
        _ => false,
    });

    assert!(has_foo, "LIST #foo* should include #foo1");
    assert!(!has_bar, "LIST #foo* should not include #bar2");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_who_command_channel() {
    let port = 16692;
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

    // Both join channel
    alice.join("#who").await.expect("Alice join failed");
    bob.join("#who").await.expect("Bob join failed");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // Alice sends WHO for the channel
    alice
        .send_raw("WHO #who")
        .await
        .expect("Failed to send WHO");

    // Expect RPL_WHOREPLY (352) and RPL_ENDOFWHO (315)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 315))
        .await
        .expect("Failed to receive WHO response");

    // Should have entries for both alice and bob
    let has_alice = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 352 => {
            params.iter().any(|p| p.contains("alice"))
        }
        _ => false,
    });

    let has_bob = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 352 => {
            params.iter().any(|p| p.contains("bob"))
        }
        _ => false,
    });

    assert!(has_alice, "WHO #who should include alice");
    assert!(has_bob, "WHO #who should include bob");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_who_command_nick() {
    let port = 16693;
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

    // Alice sends WHO for bob's nick
    alice.send_raw("WHO bob").await.expect("Failed to send WHO");

    // Expect RPL_WHOREPLY (352) and RPL_ENDOFWHO (315)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 315))
        .await
        .expect("Failed to receive WHO response");

    // Should have entry for bob
    let has_bob = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 352 => {
            params.iter().any(|p| p.contains("bob"))
        }
        _ => false,
    });

    assert!(has_bob, "WHO bob should include bob's info");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_whowas_command() {
    let port = 16694;
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

    // Bob quits (to enter WHOWAS history)
    bob.quit(Some("gone".to_string()))
        .await
        .expect("Bob quit failed");

    // Give server time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Alice sends WHOWAS for bob
    alice
        .send_raw("WHOWAS bob")
        .await
        .expect("Failed to send WHOWAS");

    // Expect RPL_WHOWASUSER (314) and RPL_ENDOFWHOWAS (369)
    let messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 369))
        .await
        .expect("Failed to receive WHOWAS response");

    // Should have WHOWAS entry for bob
    let has_bob = messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 314 => {
            params.iter().any(|p| p.contains("bob"))
        }
        _ => false,
    });

    assert!(has_bob, "WHOWAS bob should include bob's history");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}
