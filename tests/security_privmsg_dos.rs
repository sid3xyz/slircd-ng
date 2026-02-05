use crate::common::TestServer;
use slirc_proto::{Command, Response};

mod common;

#[tokio::test]
async fn test_privmsg_many_targets() {
    let server = TestServer::spawn(7000).await.expect("Failed to spawn server");
    let mut client = server.connect("test").await.expect("Failed to connect");
    client.register().await.expect("Failed to register");

    // Drain welcome burst until End of MOTD (376) or ERR_NOMOTD (422)
    let _ = client.recv_until(|msg| {
        matches!(&msg.command, Command::Response(resp, _)
            if resp.code() == 376 || resp.code() == 422)
    }).await.expect("Failed to drain welcome burst");

    // Create a comma-separated list of 20 targets (channels/users)
    let targets: Vec<String> = (0..20).map(|i| format!("#chan{}", i)).collect();
    let target_str = targets.join(",");

    client.send_raw(&format!("PRIVMSG {} :hello", target_str)).await.expect("Failed to send");

    // Fixed behavior (SECURE): The server rejects the command with ERR_TOOMANYTARGETS (407).

    // We expect one ERR_TOOMANYTARGETS.
    let msgs = client.recv_until(|msg| {
        match &msg.command {
             Command::Response(resp, _) => {
                 resp == &Response::ERR_TOOMANYTARGETS || resp == &Response::ERR_NOSUCHCHANNEL
             }
             _ => false
        }
    }).await.expect("Failed to receive response");

    let last_msg = msgs.last().unwrap();
    if let Command::Response(resp, params) = &last_msg.command {
        if resp == &Response::ERR_NOSUCHCHANNEL {
             panic!("Vulnerable! Received ERR_NOSUCHCHANNEL, meaning it processed targets.");
        }
        assert_eq!(resp, &Response::ERR_TOOMANYTARGETS, "Expected ERR_TOOMANYTARGETS");
        // We used target_list[0] as the arg
        assert_eq!(params[1], "#chan0", "Should reference the first target");
        assert!(params[2].contains("Too many recipients"), "Should have correct error message");
    } else {
        panic!("Unexpected message: {:?}", last_msg);
    }
}
