//! Integration tests for server query commands (ADMIN, INFO, TIME, VERSION, STATS, MOTD, LUSERS)

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::time::Duration;

#[tokio::test]
async fn test_version_command() {
    let port = 16690;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send VERSION command
    client
        .send_raw("VERSION")
        .await
        .expect("Failed to send VERSION");

    // Expect RPL_VERSION (351)
    let messages = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 351))
        .await
        .expect("RPL_VERSION expected");

    let version_msg = messages
        .iter()
        .find(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 351))
        .expect("VERSION response not found");

    if let Command::Response(_, params) = &version_msg.command {
        assert!(
            !params.is_empty(),
            "VERSION response should have parameters"
        );
    }
}

#[tokio::test]
async fn test_time_command() {
    let port = 16691;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send TIME command
    client.send_raw("TIME").await.expect("Failed to send TIME");

    // Expect RPL_TIME (391)
    let messages = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 391))
        .await
        .expect("RPL_TIME expected");

    assert!(
        messages
            .iter()
            .any(|m| { matches!(&m.command, Command::Response(resp, _) if resp.code() == 391) })
    );
}

#[tokio::test]
async fn test_info_command() {
    let port = 16692;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send INFO command
    client.send_raw("INFO").await.expect("Failed to send INFO");

    // Expect RPL_INFO (371) lines ending with RPL_ENDOFINFO (374)
    let mut info_count = 0;
    loop {
        let msg = client.recv_timeout(Duration::from_millis(500)).await;
        match msg {
            Ok(m) => match &m.command {
                Command::Response(resp, _) if resp.code() == 371 => {
                    info_count += 1;
                }
                Command::Response(resp, _) if resp.code() == 374 => {
                    break;
                }
                _ => continue,
            },
            Err(_) => panic!("INFO command timed out without RPL_ENDOFINFO"),
        }
    }

    assert!(
        info_count > 0,
        "INFO should return at least one RPL_INFO line"
    );
}

#[tokio::test]
async fn test_admin_command() {
    let port = 16693;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send ADMIN command
    client
        .send_raw("ADMIN")
        .await
        .expect("Failed to send ADMIN");

    // Expect RPL_ADMINME (256) first
    let messages = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 256))
        .await
        .expect("RPL_ADMINME expected");

    assert!(
        messages
            .iter()
            .any(|m| { matches!(&m.command, Command::Response(resp, _) if resp.code() == 256) })
    );

    // Drain remaining admin replies (257, 258, 259)
    tokio::time::sleep(Duration::from_millis(50)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}
}

#[tokio::test]
async fn test_motd_command() {
    let port = 16694;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send MOTD command
    client.send_raw("MOTD").await.expect("Failed to send MOTD");

    // Expect RPL_MOTDSTART (375) or ERR_NOMOTD (422)
    let messages = client
        .recv_until(|msg| {
            matches!(&msg.command, Command::Response(resp, _) if resp.code() == 375 || resp.code() == 422)
        })
        .await
        .expect("MOTD response expected");

    let first_msg = messages.iter().find(|m| {
        matches!(&m.command, Command::Response(resp, _) if resp.code() == 375 || resp.code() == 422)
    }).expect("MOTD response not found");

    if let Command::Response(resp, _) = &first_msg.command
        && resp.code() == 375
    {
        // If MOTD exists, drain RPL_MOTD (372) until RPL_ENDOFMOTD (376)
        let mut motd_lines = 0;
        loop {
            let msg = client.recv_timeout(Duration::from_millis(500)).await;
            match msg {
                Ok(m) => match &m.command {
                    Command::Response(r, _) if r.code() == 372 => {
                        motd_lines += 1;
                    }
                    Command::Response(r, _) if r.code() == 376 => {
                        break;
                    }
                    _ => continue,
                },
                Err(_) => panic!("MOTD timed out without RPL_ENDOFMOTD"),
            }
        }
        assert!(motd_lines > 0, "MOTD should have at least one line");
    }
    // ERR_NOMOTD (422) is valid - no MOTD configured
}

#[tokio::test]
async fn test_lusers_command() {
    let port = 16695;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send LUSERS command
    client
        .send_raw("LUSERS")
        .await
        .expect("Failed to send LUSERS");

    // Expect RPL_LUSERCLIENT (251)
    let messages = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 251))
        .await
        .expect("RPL_LUSERCLIENT expected");

    assert!(
        messages
            .iter()
            .any(|m| { matches!(&m.command, Command::Response(resp, _) if resp.code() == 251) })
    );

    // Drain remaining LUSER replies until RPL_LUSERME (255)
    let _luserme = client
        .recv_until(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 255))
        .await
        .expect("RPL_LUSERME expected");
}

#[tokio::test]
async fn test_stats_u_command() {
    let port = 16696;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send STATS u command (uptime)
    client
        .send_raw("STATS u")
        .await
        .expect("Failed to send STATS u");

    // Expect RPL_STATSUPTIME (242) or RPL_ENDOFSTATS (219)
    let messages = client
        .recv_until(|msg| {
            matches!(&msg.command, Command::Response(resp, _) if resp.code() == 242 || resp.code() == 219)
        })
        .await
        .expect("STATS response expected");

    // Should get either uptime or end of stats
    assert!(messages.iter().any(|m| {
        matches!(&m.command, Command::Response(resp, _) if resp.code() == 242 || resp.code() == 219)
    }));

    // Drain until RPL_ENDOFSTATS (219)
    if !messages
        .iter()
        .any(|m| matches!(&m.command, Command::Response(resp, _) if resp.code() == 219))
    {
        let _end = client
            .recv_until(
                |msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 219),
            )
            .await
            .expect("RPL_ENDOFSTATS expected");
    }
}

#[tokio::test]
async fn test_stats_l_command() {
    let port = 16697;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    let mut client = TestClient::connect(&server.address(), "alice")
        .await
        .expect("Failed to connect");

    client.register().await.expect("Registration failed");

    // Drain welcome burst
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client.recv_timeout(Duration::from_millis(10)).await.is_ok() {}

    // Send STATS l command (connections)
    client
        .send_raw("STATS l")
        .await
        .expect("Failed to send STATS l");

    // Expect RPL_STATSLINKINFO (211) and/or RPL_ENDOFSTATS (219)
    loop {
        let msg = client.recv_timeout(Duration::from_millis(500)).await;
        match msg {
            Ok(m) => match &m.command {
                // Optionally observe link info replies (211)
                Command::Response(resp, _) if resp.code() == 211 => {
                    // ignore, just draining
                }
                // End of stats (219) terminates the loop
                Command::Response(resp, _) if resp.code() == 219 => {
                    break;
                }
                _ => continue,
            },
            Err(_) => panic!("STATS l timed out without RPL_ENDOFSTATS"),
        }
    }
}
