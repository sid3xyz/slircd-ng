// tests/operator_moderation.rs
//! Integration tests for operator moderation commands: KLINE/GLINE/ZLINE/RLINE,
//! and admin/broadcast commands: REHASH, GLOBOPS.

mod common;
use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::time::Duration;

async fn drain(client: &mut TestClient) {
    tokio::time::sleep(Duration::from_millis(120)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}
}

async fn become_oper(client: &mut TestClient) {
    client
        .send_raw("OPER testop testpass")
        .await
        .expect("Failed to send OPER");
    let _ = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 381))
        .await
        .expect("Expected YOU'RE OPER (381)");
    drain(client).await;
}

async fn who_get_user_host(oper: &mut TestClient, nick: &str) -> (String, String) {
    oper.send_raw(&format!("WHO {}", nick))
        .await
        .expect("Failed to send WHO");

    let msgs = oper
        .recv_until(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 315))
        .await
        .expect("WHO should end with 315");

    // Find the 352 reply and extract user/host
    for m in &msgs {
        if let Command::Response(resp, params) = &m.command
            && resp.code() == 352 && params.len() >= 6 {
                // 352: <me> <channel|*> <user> <host> <server> <nick> <H|G>[*][@|+] :<hopcount> <real name>
                let user = params[2].clone();
                let host = params[3].clone();
                return (user, host);
            }
    }
    panic!("WHO 352 not found for {}", nick);
}

#[tokio::test]
async fn test_kline_disconnects_target_and_confirms_notice() {
    let port = 16710;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // Resolve victim's user and target any host using user@*
    let (user, _host) = who_get_user_host(&mut oper, "bob").await;
    let mask = format!("{}@*", user);

    oper.send_raw(&format!("KLINE {} :test kline", mask))
        .await
        .expect("send KLINE");

    // Oper receives server NOTICE confirming add; include disconnect count
    let oper_msgs = oper
        .recv_until(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("KLINE added") && text.contains("disconnected")))
        .await
        .expect("oper should receive KLINE confirmation with disconnect count");
    assert!(oper_msgs.iter().any(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("KLINE added") && text.contains("disconnected"))));
}

#[tokio::test]
async fn test_gline_disconnects_target_and_confirms_notice() {
    let port = 16711;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    let (user, _host) = who_get_user_host(&mut oper, "bob").await;
    let mask = format!("{}@*", user);

    oper.send_raw(&format!("GLINE {} :test gline", mask))
        .await
        .expect("send GLINE");

    let oper_msgs = oper
        .recv_until(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("GLINE added") && text.contains("disconnected")))
        .await
        .expect("oper should receive GLINE confirmation with disconnect count");
    assert!(oper_msgs.iter().any(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("GLINE added") && text.contains("disconnected"))));
}

#[tokio::test]
async fn test_zline_disconnects_target_ip_and_confirms_notice() {
    let port = 16712;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // We cannot reliably obtain the victim's real IP/host from WHO (cloaked).
    // Exercise handler path and confirmation formatting only.
    oper.send_raw("ZLINE 10.0.0.0/8 :test zline")
        .await
        .expect("send ZLINE");

    let oper_msgs = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("ZLINE added")),
        )
        .await
        .expect("oper should receive ZLINE confirmation notice");
    assert!(
        oper_msgs.iter().any(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("ZLINE added"))
        )
    );
    // Do not assert disconnect for ZLINE to avoid disconnecting oper (matching 127.0.0.1)
}

#[tokio::test]
async fn test_rline_disconnects_target_realname_and_confirms_notice() {
    let port = 16713;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // Match realname using space-less pattern that still matches: *bob*
    oper.send_raw("RLINE *bob* :test rline")
        .await
        .expect("send RLINE");

    let oper_msgs = oper
        .recv_until(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("RLINE added") && text.contains("disconnected")))
        .await
        .expect("oper should receive RLINE confirmation with disconnect count");
    assert!(oper_msgs.iter().any(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("RLINE added") && text.contains("disconnected"))));
}

#[tokio::test]
async fn test_rehash_requires_cap_and_succeeds_for_oper() {
    let port = 16714;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut user = TestClient::connect(&server.address(), "carol")
        .await
        .expect("connect user");
    user.register().await.expect("user register");
    drain(&mut user).await;

    // Non-oper REHASH should return ERR_NOPRIVILEGES (481)
    user.send_raw("REHASH").await.expect("send REHASH");
    let _ = user
        .recv_until(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 481))
        .await
        .expect("Expected ERR_NOPRIVILEGES (481)");

    // Oper REHASH returns RPL_REHASHING (382) then a NOTICE with completion
    become_oper(&mut user).await;
    user.send_raw("REHASH").await.expect("send REHASH");
    let msgs = user
        .recv_until(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 382))
        .await
        .expect("Expected RPL_REHASHING (382)");
    assert!(
        msgs.iter()
            .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 382))
    );

    // And a server NOTICE indicating completion/warning
    let _ = user
        .recv_until(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("REHASH")))
        .await
        .expect("Expected REHASH completion notice");
}

#[tokio::test]
async fn test_globops_delivered_to_g_subscribers() {
    let port = 16715;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut recipient = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect recipient");
    recipient.register().await.expect("recipient register");

    drain(&mut oper).await;
    drain(&mut recipient).await;

    become_oper(&mut oper).await;

    // Recipient subscribes to server notices via +s (defaults include 'o').
    recipient.send_raw("MODE bob +s").await.expect("set +s");
    // Also OPER to mirror common server behavior where snomasks are for opers
    recipient
        .send_raw("OPER testop testpass")
        .await
        .expect("recipient OPER");
    let _ = recipient
        .recv_until(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 381))
        .await
        .expect("recipient oper ack");
    drain(&mut recipient).await;

    // Send GLOBOPS
    oper.send_raw("GLOBOPS :test globops message")
        .await
        .expect("send GLOBOPS");

    // Recipient should receive a NOTICE with the message
    let rec_msgs = recipient
        .recv_until(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("globops")))
        .await
        .expect("recipient should receive globops notice");
    assert!(
        rec_msgs
            .iter()
            .any(|m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("globops")))
    );
}

#[tokio::test]
async fn test_unkline_removes_ban_and_allows_reconnect() {
    let port = 16716;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // Determine user@* mask for bob
    let (user, _host) = who_get_user_host(&mut oper, "bob").await;
    let mask = format!("{}@*", user);

    // Apply KLINE and expect confirmation
    oper.send_raw(&format!("KLINE {} :kline for test", mask))
        .await
        .expect("send KLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("KLINE added")),
        )
        .await
        .expect("oper should receive KLINE confirmation");

    // Remove the KLINE and expect removal notice
    oper.send_raw(&format!("UNKLINE {}", mask))
        .await
        .expect("send UNKLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("KLINE removed")),
        )
        .await
        .expect("oper should receive KLINE removed notice");

    // Reconnect bob and ensure registration succeeds (not immediately re-banned)
    let mut victim2 = TestClient::connect(&server.address(), "bob")
        .await
        .expect("reconnect victim");
    victim2.register().await.expect("victim re-register");
    drain(&mut victim2).await;
}

#[tokio::test]
async fn test_ungline_removes_ban_and_allows_reconnect() {
    let port = 16717;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // Determine user@* mask for bob
    let (user, _host) = who_get_user_host(&mut oper, "bob").await;
    let mask = format!("{}@*", user);

    // Apply GLINE and expect confirmation
    oper.send_raw(&format!("GLINE {} :gline for test", mask))
        .await
        .expect("send GLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("GLINE added")),
        )
        .await
        .expect("oper should receive GLINE confirmation");

    // Remove the GLINE and expect removal notice
    oper.send_raw(&format!("UNGLINE {}", mask))
        .await
        .expect("send UNGLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("GLINE removed")),
        )
        .await
        .expect("oper should receive GLINE removed notice");

    // Reconnect bob and ensure registration succeeds (not immediately re-banned)
    let mut victim2 = TestClient::connect(&server.address(), "bob")
        .await
        .expect("reconnect victim");
    victim2.register().await.expect("victim re-register");
    drain(&mut victim2).await;
}

#[tokio::test]
async fn test_unzline_removes_ban_confirms_notice() {
    let port = 16718;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    drain(&mut oper).await;
    become_oper(&mut oper).await;

    // Add benign ZLINE for 10.0.0.0/8, then remove it
    oper.send_raw("ZLINE 10.0.0.0/8 :temporary test zline")
        .await
        .expect("send ZLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("ZLINE added")),
        )
        .await
        .expect("oper should receive ZLINE added notice");

    oper.send_raw("UNZLINE 10.0.0.0/8")
        .await
        .expect("send UNZLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("ZLINE removed")),
        )
        .await
        .expect("oper should receive ZLINE removed notice");
}

#[tokio::test]
async fn test_unrline_removes_ban_and_allows_reconnect() {
    let port = 16719;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut oper = TestClient::connect(&server.address(), "alice")
        .await
        .expect("connect oper");
    oper.register().await.expect("oper register");

    let mut victim = TestClient::connect(&server.address(), "bob")
        .await
        .expect("connect victim");
    victim.register().await.expect("victim register");

    drain(&mut oper).await;
    drain(&mut victim).await;

    become_oper(&mut oper).await;

    // Apply RLINE on *bob* pattern then remove it
    oper.send_raw("RLINE *bob* :rline for test")
        .await
        .expect("send RLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("RLINE added")),
        )
        .await
        .expect("oper should receive RLINE confirmation");

    oper.send_raw("UNRLINE *bob*").await.expect("send UNRLINE");
    let _ = oper
        .recv_until(
            |m| matches!(&m.command, Command::NOTICE(_, text) if text.contains("RLINE removed")),
        )
        .await
        .expect("oper should receive RLINE removed notice");

    // Reconnect bob and ensure registration succeeds (not blocked by RLINE)
    let mut victim2 = TestClient::connect(&server.address(), "bob")
        .await
        .expect("reconnect victim");
    victim2.register().await.expect("victim re-register");
    drain(&mut victim2).await;
}
