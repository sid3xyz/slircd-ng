//! Integration tests for channel operations: PART and TOPIC.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;

#[tokio::test]
async fn test_part_broadcast() {
    let port = 16673;
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
    alice.join("#ops").await.expect("Alice join failed");
    bob.join("#ops").await.expect("Bob join failed");

    // Drain JOIN responses to ensure both are fully in the channel
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
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

    // Alice parts the channel
    alice
        .part("#ops", Some("bye"))
        .await
        .expect("Alice part failed");

    // Bob should receive a PART for #ops
    let messages = bob
        .recv_until(|msg| matches!(&msg.command, Command::PART(chan, _reason) if chan == "#ops"))
        .await
        .expect("Bob failed to receive PART");

    assert!(messages.iter().any(|m| match &m.command {
        Command::PART(chan, reason) => chan == "#ops" && reason.as_deref() == Some("bye"),
        _ => false,
    }));

    // Cleanly disconnect
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_topic_broadcast() {
    let port = 16674;
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

    // Join channel sequentially to ensure both are fully in channel
    alice.join("#ops").await.expect("Alice join failed");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    bob.join("#ops").await.expect("Bob join failed");

    // Drain JOIN responses
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
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

    // Alice sets topic
    alice
        .topic("#ops", "integration testing topic")
        .await
        .expect("Alice topic failed");

    // Bob should receive a TOPIC for #ops
    let messages = bob
        .recv_until(|msg| matches!(&msg.command, Command::TOPIC(chan, Some(text)) if chan == "#ops" && text.contains("integration")))
        .await
        .expect("Bob failed to receive TOPIC");

    assert!(messages.iter().any(|m| match &m.command {
        Command::TOPIC(chan, Some(text)) => chan == "#ops" && text == "integration testing topic",
        _ => false,
    }));

    // Cleanly disconnect
    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_invite_flow() {
    let port = 16675;
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

    // Alice invites Bob to #invite (channel may not exist yet; RFC allows this)
    alice
        .send(Command::INVITE("#invite".to_string(), "bob".to_string()))
        .await
        .expect("Alice INVITE failed");

    // Bob should receive an INVITE for #invite
    let messages = bob
        .recv_until(|msg| match &msg.command {
            Command::INVITE(a, b) => {
                (a == "#invite" && b == "bob") || (a == "bob" && b == "#invite")
            }
            _ => false,
        })
        .await
        .expect("Bob failed to receive INVITE");

    assert!(messages.iter().any(|m| match &m.command {
        Command::INVITE(a, b) => (a == "#invite" && b == "bob") || (a == "bob" && b == "#invite"),
        _ => false,
    }));

    // Bob joins #invite; check that join completes without error
    bob.join("#invite").await.expect("Bob join failed");

    // Cleanly disconnect
    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}

#[tokio::test]
async fn test_kick_requires_op_and_succeeds_with_op() {
    let port = 16676;
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

    // Bob joins first and gets +o; alice joins without +o
    bob.join("#ops").await.expect("Bob join failed");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    alice.join("#ops").await.expect("Alice join failed");

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

    // Alice attempts KICK without +o; expect 482 (ERR_CHANOPRIVSNEEDED)
    alice
        .send_raw("KICK #ops bob :testing")
        .await
        .expect("Alice KICK send failed");
    let _ = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 482))
        .await
        .expect("Alice did not receive ERR_CHANOPRIVSNEEDED");

    // Bob (who has +o) grants alice +o, then alice can KICK
    bob.mode_channel_op("#ops", "alice")
        .await
        .expect("Bob MODE +o alice failed");

    // Drain MODE responses
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    alice
        .send_raw("KICK #ops bob :testing")
        .await
        .expect("Alice KICK send failed");

    let _ = bob
        .recv_until(|msg| matches!(&msg.command, Command::KICK(chan, target, _reason) if chan == "#ops" && target == "bob"))
        .await
        .expect("Bob did not receive KICK");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
}

#[tokio::test]
async fn test_names_and_whois() {
    let port = 16677;
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

    alice.join("#ops").await.expect("Alice join failed");
    bob.join("#ops").await.expect("Bob join failed");

    // Drain automatic NAMES responses from JOIN
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    while alice
        .recv_timeout(tokio::time::Duration::from_millis(10))
        .await
        .is_ok()
    {}

    // NAMES: expect 353 (RPL_NAMREPLY) and 366 (RPL_ENDOFNAMES)
    // Format: :server 353 <client> <symbol> <channel> :<names>
    // Params: [client, symbol, channel, names_list]
    alice
        .send_raw("NAMES #ops")
        .await
        .expect("Alice NAMES send failed");
    let names_messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 366))
        .await
        .expect("Alice did not receive END OF NAMES");

    // Find RPL_NAMREPLY and check it contains both nicks
    let has_names = names_messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 353 => {
            // params should be ["alice", "=", "#ops", "@alice bob"] or similar
            params.len() >= 4
                && params.iter().any(|p| p == "#ops")
                && params
                    .last()
                    .map(|names| names.contains("alice") && names.contains("bob"))
                    .unwrap_or(false)
        }
        _ => false,
    });
    assert!(has_names, "NAMES response should list alice and bob");

    // WHOIS bob: expect 311 (RPL_WHOISUSER) and 318 (RPL_ENDOFWHOIS)
    alice
        .send_raw("WHOIS bob")
        .await
        .expect("Alice WHOIS send failed");
    let whois_messages = alice
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 318))
        .await
        .expect("Alice did not receive END OF WHOIS");

    let has_whois = whois_messages.iter().any(|m| match &m.command {
        Command::Response(resp, params) if resp.code() == 311 => {
            params.iter().any(|p| p.contains("bob"))
        }
        _ => false,
    });
    assert!(has_whois, "WHOIS response should contain bob's info");

    alice
        .quit(Some("done".to_string()))
        .await
        .expect("Alice quit failed");
    bob.quit(Some("done".to_string()))
        .await
        .expect("Bob quit failed");
}
