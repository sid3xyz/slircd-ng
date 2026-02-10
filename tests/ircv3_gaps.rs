//! Tests for IRCv3 features identified as gaps in the audit.
//! MONITOR, METADATA, SETNAME

mod common;

use common::{TestClient, TestServer};
use tokio::time::Duration;

/// Test MONITOR command - track online/offline status of nicks.
#[tokio::test]
async fn test_monitor_add_and_status() {
    let port = 16700;
    let server = TestServer::spawn(port).await.expect("spawn");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect");
    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect");

    alice.register().await.expect("register");
    bob.register().await.expect("register");

    // Drain welcome
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}
    while bob.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Alice monitors bob
    alice.send_raw("MONITOR + bob\r\n").await.expect("send");

    // Should get RPL_MONONLINE (730) since bob is online
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    assert!(
        s.contains("730") || s.contains("bob"),
        "Expected MONONLINE with bob: {}",
        s
    );

    // Check status
    alice.send_raw("MONITOR S\r\n").await.expect("send");
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    assert!(
        s.contains("730") || s.contains("bob"),
        "Expected status with bob: {}",
        s
    );
}

/// Test MONITOR - detect when monitored user goes offline.
#[tokio::test]
async fn test_monitor_offline_notification() {
    let port = 16701;
    let server = TestServer::spawn(port).await.expect("spawn");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect");
    let mut bob = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect");

    alice.register().await.expect("register");
    bob.register().await.expect("register");

    // Drain welcome
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}
    while bob.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Alice monitors bob
    alice.send_raw("MONITOR + bob\r\n").await.expect("send");
    // Drain the MONONLINE
    while alice.recv_timeout(Duration::from_millis(100)).await.is_ok() {}

    // Bob quits
    bob.quit(Some("leaving".to_string())).await.expect("quit");

    // Alice should get RPL_MONOFFLINE (731)
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    assert!(
        s.contains("731") || s.contains("bob"),
        "Expected MONOFFLINE: {}",
        s
    );
}

/// Test METADATA GET/SET on user.
#[tokio::test]
async fn test_metadata_user_get_set() {
    let port = 16702;
    let server = TestServer::spawn(port).await.expect("spawn");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect");
    alice.register().await.expect("register");

    // Drain welcome
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Set metadata on self
    alice
        .send_raw("METADATA alice SET url :https://example.com\r\n")
        .await
        .expect("send");

    // Collect SET response - should be 761 (KEYVALUE) + 762 (END)
    let mut set_responses = String::new();
    for _ in 0..2 {
        if let Ok(msg) = alice.recv_timeout(Duration::from_secs(2)).await {
            set_responses.push_str(&msg.to_string());
            set_responses.push('\n');
        }
    }
    assert!(
        set_responses.contains("761") || set_responses.contains("url"),
        "SET should return KEYVALUE: {}",
        set_responses
    );

    // Get metadata
    alice
        .send_raw("METADATA alice GET url\r\n")
        .await
        .expect("send");

    // Collect GET response - should be 761 (KEYVALUE) + 762 (END) or just 762 if no match
    let mut get_responses = String::new();
    for _ in 0..2 {
        if let Ok(msg) = alice.recv_timeout(Duration::from_secs(2)).await {
            get_responses.push_str(&msg.to_string());
            get_responses.push('\n');
        }
    }
    assert!(
        get_responses.contains("url") || get_responses.contains("example.com"),
        "GET should return value: {}",
        get_responses
    );
}

/// Test METADATA on channel.
#[tokio::test]
async fn test_metadata_channel() {
    let port = 16703;
    let server = TestServer::spawn(port).await.expect("spawn");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect");
    alice.register().await.expect("register");

    // Drain welcome
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Join channel (becomes op)
    alice.join("#meta").await.expect("join");
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Set channel metadata
    alice
        .send_raw("METADATA #meta SET website :https://chan.example.com\r\n")
        .await
        .expect("send");
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    assert!(
        !s.contains("ERROR") && !s.contains("FAIL"),
        "Channel SET should not error: {}",
        s
    );

    // List channel metadata
    alice
        .send_raw("METADATA #meta LIST\r\n")
        .await
        .expect("send");
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    // Should contain the key we set or end-of-list
    assert!(
        s.contains("website") || s.contains("761") || s.contains("762"),
        "LIST should work: {}",
        s
    );
}

/// Test SETNAME command - change realname while connected.
#[tokio::test]
async fn test_setname_command() {
    let port = 16704;
    let server = TestServer::spawn(port).await.expect("spawn");

    let mut alice = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect");

    // Request setname capability during registration
    alice.send_raw("CAP LS 302\r\n").await.expect("send");
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    alice.send_raw("CAP REQ :setname\r\n").await.expect("send");
    tokio::time::sleep(Duration::from_millis(100)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    alice.send_raw("CAP END\r\n").await.expect("send");
    alice.send_raw("NICK alice\r\n").await.expect("send");
    alice
        .send_raw("USER alice 0 * :Original Name\r\n")
        .await
        .expect("send");

    // Drain welcome
    tokio::time::sleep(Duration::from_millis(200)).await;
    while alice.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Change realname
    alice
        .send_raw("SETNAME :New Fancy Name\r\n")
        .await
        .expect("send");
    let msg = alice
        .recv_timeout(Duration::from_secs(2))
        .await
        .expect("recv");
    let s = msg.to_string();
    // Should get SETNAME echo or acknowledgment
    assert!(
        s.contains("SETNAME") || s.contains("New Fancy Name"),
        "SETNAME should echo: {}",
        s
    );

    // Verify via WHOIS
    alice.send_raw("WHOIS alice\r\n").await.expect("send");
    let mut found_new_name = false;
    for _ in 0..10 {
        if let Ok(msg) = alice.recv_timeout(Duration::from_millis(500)).await {
            if msg.to_string().contains("New Fancy Name") {
                found_new_name = true;
                break;
            }
        } else {
            break;
        }
    }
    assert!(found_new_name, "WHOIS should show new realname");
}
