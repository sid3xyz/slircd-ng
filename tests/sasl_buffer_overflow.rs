use std::time::Duration;

mod common;
use common::{TestClient, TestServer};

#[tokio::test]
async fn test_sasl_buffer_overflow() {
    // 1. Spawn test server
    let port = 17999;
    let server = TestServer::spawn(port)
        .await
        .expect("Failed to spawn server");

    // 2. Connect client
    let mut client = TestClient::connect(&server.address(), "overflow")
        .await
        .expect("Connect failed");

    // 3. Initiate SASL
    client
        .send_raw("CAP REQ :sasl\r\n")
        .await
        .expect("CAP REQ failed");

    // Consume CAP ACK - use manual loop since recv_until might not be available or behaves differently
    let mut cap_ack = false;
    for _ in 0..10 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await {
            if msg.to_string().contains("CAP") && msg.to_string().contains("ACK") {
                cap_ack = true;
                break;
            }
        }
    }
    assert!(cap_ack, "Did not receive CAP ACK");

    // 4. Start AUTHENTICATE PLAIN
    client
        .send_raw("AUTHENTICATE PLAIN\r\n")
        .await
        .expect("AUTH PLAIN failed");

    // Wait for challenge (+)
    let mut challenge = false;
    for _ in 0..10 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await {
            if msg.to_string().contains("AUTHENTICATE +") {
                challenge = true;
                break;
            }
        }
    }
    assert!(challenge, "Did not receive challenge");

    // 5. Send payload exceeding 16KB in chunks
    // We'll send 50 chunks of 400 bytes = 20000 bytes.
    let chunk = "A".repeat(400);

    for _ in 0..50 {
        client
            .send_raw(&format!("AUTHENTICATE {}\r\n", chunk))
            .await
            .expect("Chunk send failed");
    }

    // 6. Verification
    // Expect 904 (SASL Fail) or Error within a timeout.
    // If we timeout, it means the server is silently buffering (Vulnerable).

    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(msg) = client.recv().await {
                let s = msg.to_string();
                // 904 = ERR_SASLFAIL
                if s.contains("904") || s.contains("fail") || s.contains("abort") {
                    return Ok::<(), anyhow::Error>(());
                }
            } else {
                // Connection closed or error
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await;

    match result {
        Ok(Ok(())) => {
            println!("SUCCESS: Server rejected overflow");
        }
        Err(_) => {
            panic!(
                "FAILURE: Server accepted overflow (Vulnerability confirmed - Timeout waiting for rejection)"
            );
        }
        _ => panic!("Unexpected error"),
    }
}
