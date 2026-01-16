//! Stress test for concurrent SASL authentication.
//!
//! Tests that the server can handle 100+ simultaneous SASL PLAIN logins
//! without executor stalls or timeouts. This is critical for the Argon2
//! spawn_blocking fix - if the fix is incorrect, this test will timeout
//! or fail with auth errors.
//!
//! This test verifies:
//! - No async executor stalls from password hashing
//! - Database lock contention is manageable
//! - Connection stability under concurrent auth load

mod common;

use common::{TestClient, TestServer};
use slirc_proto::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[tokio::test]
async fn test_100_concurrent_sasl_plain_logins() {
    // Spawn test server
    let port = 16668;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn test server");

    // First, register some test accounts (do this serially to avoid initial contention)
    let address = server.address();
    for i in 0..10 {
        let mut client = TestClient::connect(&address, &format!("testuser{}", i))
            .await
            .expect("Failed to connect for registration");

        // Register account with password
        let register_cmd = format!(
            "PRIVMSG NickServ :REGISTER testpass{} user{}@example.com\r\n",
            i, i
        );
        client
            .send_raw(&register_cmd)
            .await
            .expect("Failed to send REGISTER");

        // Consume responses
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        while client
            .recv_timeout(tokio::time::Duration::from_millis(10))
            .await
            .is_ok()
        {}
    }

    // Now stress test: 100 concurrent SASL PLAIN authentications
    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();

    // Spawn 100 concurrent auth tasks
    let mut handles = Vec::new();

    for i in 0..100 {
        let success_count = success_count.clone();
        let failure_count = failure_count.clone();
        let address = server.address().to_string();

        let handle = tokio::spawn(async move {
            // Cycle through 10 accounts
            let account_id = i % 10;
            let account_name = format!("testuser{}", account_id);
            let password = format!("testpass{}", account_id);

            match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                perform_sasl_plain_auth(&address, &account_name, &password),
            )
            .await
            {
                Ok(Ok(_)) => {
                    success_count.fetch_add(1, Ordering::Relaxed);
                }
                Ok(Err(e)) => {
                    eprintln!("Auth failed for {}: {}", account_name, e);
                    failure_count.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    eprintln!("Timeout for {}", account_name);
                    failure_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        let _ = handle.await;
    }

    let elapsed = start.elapsed();
    let successes = success_count.load(Ordering::Relaxed);
    let failures = failure_count.load(Ordering::Relaxed);

    println!("Concurrent SASL stress test completed in {:?}", elapsed);
    println!("  Successes: {}/100", successes);
    println!("  Failures: {}/100", failures);
    println!("  Time per auth: {:?}", elapsed / 100u32);

    // Assert success rate (allow 1-2 failures due to test infrastructure)
    assert!(
        successes >= 98,
        "Expected at least 98 successful auths, got {} (failures: {})",
        successes,
        failures
    );

    // Assert reasonable time (should complete well under 30 seconds)
    assert!(
        elapsed.as_secs() < 30,
        "Stress test took too long: {:?} - indicates executor stalls",
        elapsed
    );

    println!("✓ Stress test PASSED - server handles 100 concurrent SASL logins");
}

/// Perform a single SASL PLAIN authentication against the test server.
async fn perform_sasl_plain_auth(
    address: &str,
    account: &str,
    password: &str,
) -> anyhow::Result<()> {
    let mut client = TestClient::connect(address, "stresstest").await?;

    // Send CAP REQ SASL to begin SASL flow
    client.send_raw("CAP REQ :sasl\r\n").await?;

    // Wait for CAP ACK
    loop {
        let msg = client
            .recv_timeout(std::time::Duration::from_secs(5))
            .await?;
        if msg.to_string().contains("CAP") && msg.to_string().contains("ACK") {
            break;
        }
    }

    // Initiate SASL PLAIN
    client.send_raw(&format!("AUTHENTICATE PLAIN\r\n")).await?;

    // Wait for AUTHENTICATE + prompt
    loop {
        let msg = client
            .recv_timeout(std::time::Duration::from_secs(5))
            .await?;
        if msg.to_string().contains("AUTHENTICATE") {
            break;
        }
    }

    // Send SASL PLAIN credentials (base64 encoded: account\0account\0password)
    let credentials = format!("{}\0{}\0{}", account, account, password);
    let encoded = base64_encode(credentials.as_bytes());
    client
        .send_raw(&format!("AUTHENTICATE {}\r\n", encoded))
        .await?;

    // Wait for success or failure
    let mut success = false;
    for _ in 0..5 {
        match client.recv_timeout(std::time::Duration::from_secs(5)).await {
            Ok(msg) => {
                let s = msg.to_string();
                if s.contains("903") || s.contains("SASL") && s.contains("success") {
                    success = true;
                    break;
                } else if s.contains("904") || s.contains("SASL") && s.contains("fail") {
                    return Err(anyhow::anyhow!("SASL authentication failed"));
                }
            }
            Err(e) => {
                eprintln!("Error during SASL: {}", e);
                return Err(e.into());
            }
        }
    }

    if !success {
        return Err(anyhow::anyhow!("SASL did not complete"));
    }

    Ok(())
}

/// Simple base64 encoding (for SASL PLAIN credentials).
fn base64_encode(data: &[u8]) -> String {
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in data.chunks(3) {
        let b1 = chunk[0];
        let b2 = chunk.get(1).copied().unwrap_or(0);
        let b3 = chunk.get(2).copied().unwrap_or(0);

        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        result.push(BASE64_CHARS[((n >> 18) & 0x3f) as usize] as char);
        result.push(BASE64_CHARS[((n >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            result.push(BASE64_CHARS[((n >> 6) & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(n & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        // Test vector from RFC 4648
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }
}
