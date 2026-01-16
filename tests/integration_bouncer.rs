//! Integration tests for Bouncer / Multiclient features.
//!
//! Verifies synchronization between different sessions of the same account.

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;
use tokio::time::{sleep, Duration};

/// Helper to perform SASL PLAIN authentication
async fn perform_sasl_auth(
    client: &mut TestClient,
    account: &str,
    password: &str,
) -> anyhow::Result<()> {
    // 1. Request SASL capability
    client.send_raw("CAP REQ :sasl").await?;
    
    // 2. Wait for CAP ACK
    loop {
        let msg = client.recv().await?;
        // Simple check to avoid parsing headaches
        let s = msg.to_string();
        if s.contains("CAP") && s.contains("ACK") && s.contains("sasl") {
             break;
        }
        if s.contains("CAP") && s.contains("NAK") {
            anyhow::bail!("SASL CAP NAK'd");
        }
    }

    // 3. Start Authentication
    client.send_raw("AUTHENTICATE PLAIN").await?;

    // 4. Wait for +
    loop {
        let msg = client.recv().await?;
        if let Command::AUTHENTICATE(data) = &msg.command {
            if data == "+" {
                break;
            }
        }
    }


    // 5. Send Credentials
    let credentials = format!("{}\0{}\0{}", account, account, password);
    let encoded = {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD.encode(credentials)
    };
    client.send_raw(&format!("AUTHENTICATE {}", encoded)).await?;

    // 6. Wait for success (903) or failure (904)
    loop {
        let msg = client.recv().await?;
        match &msg.command {
            Command::Response(resp, _) => {
                if resp.code() == 903 { // RPL_SASLSUCCESS
                    break;
                }
                if resp.code() == 904 { // RPL_SASLFAIL
                    anyhow::bail!("SASL authentication failed (904)");
                }
            }
            _ => continue,
        }
    }

    // 7. End negotiation
    client.send_raw("CAP END").await?;
    
    Ok(())
}

#[tokio::test]
async fn test_channel_self_echo_sync() {
    let server = TestServer::spawn(19999).await.expect("Failed to spawn server");
    let address = server.address();

    // Account credentials
    let account = "bounceruser";
    let password = "passHere123";

    // -------------------------------------------------------------------------
    // Set up Account (Using Session 0)
    // -------------------------------------------------------------------------
    {
        let mut setup_client = TestClient::connect(&address, account)
            .await
            .expect("Failed to connect setup client");
        
        setup_client.register().await.expect("Register failed");
        
        // Wait for welcome
        
        // Use NickServ emulation
        setup_client.send_raw(&format!("PRIVMSG NickServ :REGISTER {} email@test.com", password)).await.expect("Send REGISTER failed");
        
        // Wait a bit
        sleep(Duration::from_millis(500)).await;
    }

    // -------------------------------------------------------------------------
    // Session A (The Sender)
    // -------------------------------------------------------------------------
    let mut client_a = TestClient::connect(&address, "SessionA")
        .await
        .expect("Failed to connect A");
    
    // Perform SASL Auth BEFORE registration
    perform_sasl_auth(&mut client_a, account, password).await.expect("Session A SASL failed");
    
    client_a.register().await.expect("Session A register failed");

    // -------------------------------------------------------------------------
    // Session B (The Sync Target)
    // -------------------------------------------------------------------------
    let mut client_b = TestClient::connect(&address, "SessionB")
        .await
        .expect("Failed to connect B");

    perform_sasl_auth(&mut client_b, account, password).await.expect("Session B SASL failed");
    
    client_b.register().await.expect("Session B register failed");

    // -------------------------------------------------------------------------
    // Channel Join
    // -------------------------------------------------------------------------
    let channel = "#sync_test";
    
    client_a.send_raw(&format!("JOIN {}", channel)).await.expect("A join failed");
    client_b.send_raw(&format!("JOIN {}", channel)).await.expect("B join failed");
    
    // Wait for joins to propagate
    sleep(Duration::from_millis(200)).await;
    
    // Clear receive buffers (ignore JOIN messages etc)
    // We can just drain until empty or ignore.
    
    // -------------------------------------------------------------------------
    // The Test: A sends, B must receive
    // -------------------------------------------------------------------------
    let msg_content = "Hello Cluster Sync";
    client_a.send(Command::PRIVMSG(channel.to_string(), msg_content.to_string()))
        .await
        .expect("A send failed");
        
    // B should receive it
    let received = client_b.recv_until(|msg| {
        match &msg.command {
            Command::PRIVMSG(target, text) => {
                target == channel && text == msg_content
            }
            _ => false
        }
    }).await.expect("B failed to receive message");
    
    let target_msg = received.last().unwrap();
    
    // Optional: Verify sender is SessionA
    if let Some(prefix) = &target_msg.prefix {
        assert!(prefix.to_string().starts_with("SessionA"), "Sender should be SessionA");
    } else {
        panic!("Message has no prefix");
    }
}

#[tokio::test]
async fn test_state_synchronization() {
    let server = TestServer::spawn(20000).await.expect("Failed to spawn server");
    let address = server.address();

    let account = "syncuser";
    let password = "syncpass123";

    // Setup Account
    {
        let mut setup = TestClient::connect(&address, account).await.unwrap();
        setup.register().await.unwrap();
        setup.send_raw(&format!("REGISTER {} email@test.com {}", account, password)).await.unwrap();
        sleep(Duration::from_millis(500)).await;
    }

    // Connect Session A
    let mut client_a = TestClient::connect(&address, "SessionA").await.unwrap();
    perform_sasl_auth(&mut client_a, account, password).await.unwrap();
    client_a.register().await.unwrap();

    // Connect Session B
    let mut client_b = TestClient::connect(&address, "SessionB").await.unwrap();
    perform_sasl_auth(&mut client_b, account, password).await.unwrap();
    client_b.register().await.unwrap();

    sleep(Duration::from_millis(200)).await;

    // 1. Test JOIN Sync
    let channel = "#slircd-dev";
    client_a.send_raw(&format!("JOIN {}", channel)).await.unwrap();

    // Session B should receive a JOIN message for SessionA (or "SessionB" if virtual?)
    // In slircd-ng, B receives the JOIN for A because A joined.
    // BUT, B should *ALSO* receive a JOIN for B!
    // Because B is now joined.
    // Wait, if B is virtually joined, B sees itself join.
    // The test asserts "Session B receives a JOIN event for #slircd-dev".
    
    // We expect B to see ITSELF join (Prefix: SessionB).
    let join_msg = client_b.recv_until(|msg| {
         if let Command::JOIN(chan, _, _) = &msg.command {
             chan == channel && msg.prefix.as_ref().unwrap().to_string().starts_with("SessionB")
         } else {
             false
         }
    }).await.expect("Session B did not see itself join via sync");

    // 2. Test NICK Sync
    // A changes nick to "Sid_Away"
    // B should see its own nick change to "Sid_Away"
    let new_nick = "Sid_Away";
    client_a.send_raw(&format!("NICK {}", new_nick)).await.unwrap();

    let nick_msg = client_b.recv_until(|msg| {
         if let Command::NICK(nick) = &msg.command {
             nick == new_nick
         } else {
             false
         }
    }).await.expect("Session B did not see its nick change");

    // 3. Test PART Sync
    client_a.send_raw(&format!("PART {}", channel)).await.unwrap();

    // Session B should see itself PART
    client_b.recv_until(|msg| {
         if let Command::PART(chan, _) = &msg.command {
             chan == channel && msg.prefix.as_ref().unwrap().to_string().starts_with("SessionB")
         } else {
             false
         }
    }).await.expect("Session B did not see itself PART via sync");
}
