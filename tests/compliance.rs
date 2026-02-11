use crate::common::TestServer;
use slirc_proto::Command;

mod common;

#[tokio::test]
async fn test_relaymsg_no_cap() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let port = listener.local_addr().expect("Failed to get addr").port();
    drop(listener);

    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn server");

    // Client without draft/relaymsg CAP should still be able to use RELAYMSG
    // (the CAP only controls whether recipients get the draft/relaymsg tag),
    // but will fail because they lack channel operator privileges.
    let mut client = server.connect("NoCap").await.expect("Failed to connect");
    client.register().await.expect("Failed to register");

    // Drain welcome burst
    loop {
        let msg = client.recv().await.expect("Failed to recv during burst");
        if let Command::Response(resp, _) = &msg.command
            && (resp.code() == 376 || resp.code() == 422) {
                break;
            }
    }

    // Attempt RELAYMSG on a channel we're not in
    client
        .send(Command::Raw(
            "RELAYMSG #test other/net :hello".to_string(),
            vec![],
        ))
        .await
        .expect("Failed to send");

    // Expect FAIL PRIVS_NEEDED (not in channel / not an op)
    let msg = client.recv().await.expect("Failed to recv");
    let is_fail = match &msg.command {
        Command::FAIL(cmd, code, _) => cmd == "RELAYMSG" && code == "PRIVS_NEEDED",
        Command::Raw(cmd, args) => {
            cmd == "FAIL"
                && args.first().is_some_and(|a| a == "RELAYMSG")
                && args.get(1).is_some_and(|a| a == "PRIVS_NEEDED")
        }
        _ => false,
    };
    assert!(
        is_fail,
        "Expected FAIL RELAYMSG PRIVS_NEEDED, got: {:?}",
        msg
    );
}

#[tokio::test]
async fn test_relaymsg_with_cap() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let port = listener.local_addr().expect("Failed to get addr").port();
    drop(listener);

    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn server");

    // 2. Client with capability
    let mut client = server.connect("WithCap").await.expect("Failed to connect");

    // Start negotiation proper
    client
        .send(Command::CAP(
            None,
            slirc_proto::CapSubCommand::LS,
            None,
            Some("302".to_string()),
        ))
        .await
        .expect("Failed LS");

    // Perform registration (NICK/USER) during negotiation
    client
        .send(Command::NICK("WithCap".to_string()))
        .await
        .expect("Failed NICK");
    client
        .send(Command::USER(
            "WithCap".to_string(),
            "0".to_string(),
            "With Cap User".to_string(),
        ))
        .await
        .expect("Failed USER");

    // Read headers until LS
    loop {
        let m = client.recv().await.expect("Failed recv LS");
        // CAP LS can return caps in arg1 or arg2 dependig on implementation details
        if let Command::CAP(_, slirc_proto::CapSubCommand::LS, arg1, arg2) = m.command {
            let caps = arg2.as_ref().or(arg1.as_ref()).expect("No caps in LS");
            assert!(
                caps.contains("draft/relaymsg"),
                "Server strictly needs to advertise draft/relaymsg. Got: {}",
                caps
            );
            break;
        }
    }

    // 4. Request the capability
    client
        .send(Command::CAP(
            None,
            slirc_proto::CapSubCommand::REQ,
            None,
            Some("draft/relaymsg".to_string()),
        ))
        .await
        .expect("Failed REQ");

    // 5. Wait for ACK
    loop {
        let m = client.recv().await.expect("Failed recv ACK");
        match m.command {
            Command::CAP(_, slirc_proto::CapSubCommand::ACK, Some(caps), _)
            | Command::CAP(_, slirc_proto::CapSubCommand::ACK, _, Some(caps)) => {
                assert!(
                    caps.contains("draft/relaymsg"),
                    "Server did not ACK draft/relaymsg"
                );
                break;
            }
            _ => continue,
        }
    }

    // 6. End negotiation
    client
        .send(Command::CAP(
            None,
            slirc_proto::CapSubCommand::END,
            None,
            None,
        ))
        .await
        .expect("Failed END");

    // 7. Wait for Welcome
    loop {
        let m = client.recv().await.expect("Failed recv Welcome");
        if let Command::Response(resp, _) = &m.command
            && resp.code() == 1 {
                break;
            }
    }
}

#[tokio::test]
async fn test_chathistory_compliance() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let port = listener.local_addr().expect("Failed to get addr").port();
    drop(listener);

    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn server");

    // 1. Setup: Alice connects and populates history
    let mut alice = server
        .connect("Alice")
        .await
        .expect("Failed to connect Alice");
    // Alice just wants to register normally
    alice.register().await.expect("Failed to register Alice");

    let channel = "#history_test";
    alice
        .send(Command::JOIN(channel.to_string(), None, None))
        .await
        .expect("Alice JOIN");

    // Send 3 messages
    for i in 1..=3 {
        alice
            .send(Command::PRIVMSG(
                channel.to_string(),
                format!("Message {}", i),
            ))
            .await
            .expect("Alice PRIVMSG");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // 2. Bob connects with chathistory capability
    let mut bob = server.connect("Bob").await.expect("Failed to connect Bob");

    bob.send(Command::CAP(
        None,
        slirc_proto::CapSubCommand::LS,
        None,
        Some("302".to_string()),
    ))
    .await
    .expect("Bob CAP LS");

    // Perform registration
    bob.send(Command::NICK("Bob".to_string()))
        .await
        .expect("Bob NICK");
    bob.send(Command::USER(
        "Bob".to_string(),
        "0".to_string(),
        "Bob User".to_string(),
    ))
    .await
    .expect("Bob USER");

    // Wait for LS
    loop {
        let m = bob.recv().await.expect("Bob recv LS");
        if let Command::CAP(_, slirc_proto::CapSubCommand::LS, _, _) = m.command {
            break;
        }
    }

    // Request chathistory
    bob.send(Command::CAP(
        None,
        slirc_proto::CapSubCommand::REQ,
        None,
        Some("draft/chathistory".to_string()),
    ))
    .await
    .expect("Bob CAP REQ");

    // Wait for ACK
    loop {
        let m = bob.recv().await.expect("Bob recv ACK");
        if let Command::CAP(_, slirc_proto::CapSubCommand::ACK, _, _) = m.command {
            break;
        }
    }

    bob.send(Command::CAP(
        None,
        slirc_proto::CapSubCommand::END,
        None,
        None,
    ))
    .await
    .expect("Bob CAP END");

    // Wait for Bob welcome
    loop {
        let m = bob.recv().await.expect("Bob recv Welcome");
        if let Command::Response(resp, _) = &m.command
            && resp.code() == 1 {
                break;
            }
    }

    // Bob must join the channel to see history
    bob.send(Command::JOIN(channel.to_string(), None, None))
        .await
        .expect("Bob JOIN");

    // 3. Bob queries history
    // CHATHISTORY LATEST <target> <limit>
    bob.send_raw(&format!("CHATHISTORY LATEST {} * 10", channel))
        .await
        .expect("Bob CHATHISTORY");

    // 4. Verify Batch response
    // Expect: BATCH +<id> chathistory <target>
    //         @batch=<id> ... PRIVMSG ...
    //         BATCH -<id>

    let mut batch_id = None;
    let mut msg_count = 0;

    loop {
        let m = bob.recv().await.expect("Bob recv history");
        match m.command {
            Command::BATCH(ref reference, ref type_arg, _) => {
                if let Some(id) = reference.strip_prefix('+') {
                    // Start of batch
                    if let Some(slirc_proto::BatchSubCommand::CUSTOM(t)) = type_arg
                        && t == "CHATHISTORY" {
                            batch_id = Some(id.to_string());
                        }
                } else if let Some(id) = reference.strip_prefix('-') {
                    // End of batch
                    assert_eq!(batch_id.as_deref(), Some(id), "Batch ID mismatch");
                    break; // Batch ended
                }
            }
            Command::PRIVMSG(ref target, ref text) => {
                // Should check if it belongs to the batch if tags handling was in TestClient
                // For now, assuming if we get PRIVMSG here it's history
                assert_eq!(target, channel);
                if text.starts_with("Message") {
                    msg_count += 1;
                }
            }
            _ => {}
        }
    }

    assert_eq!(
        msg_count, 3,
        "Bob should receive exactly 3 history messages"
    );
}
