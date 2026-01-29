use crate::common::server::TestServer;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_gateway_handshake_concurrency() {
    let port = 56667;
    // Setup custom config with Proxy Protocol enabled
    let data_dir = std::env::temp_dir().join(format!("slircd-test-proxy-{}", port));
    std::fs::create_dir_all(&data_dir).unwrap();
    let config_path = data_dir.join("config.toml");

    let config_content = format!(
        r#"
[server]
name = "test.proxy"
network = "TestNet"
sid = "00P"
description = "Proxy Test"
metrics_port = 0

[listen]
address = "127.0.0.1:{}"
proxy_protocol = true

[database]
path = "{}/test.db"

    [timeouts]
    registration_timeout = 2

[security]
cloak_secret = "secret"
cloak_suffix = "test"
spam_detection_enabled = false

[security.rate_limits]
message_rate_per_second = 1000
connection_burst_per_ip = 1000
join_burst_per_client = 1000
max_connections_per_ip = 200

[motd]
lines = ["Proxy Test"]

[[oper]]
name = "admin"
password = "pass"
host = "*@*"
"#,
        port,
        data_dir.display()
    );

    std::fs::write(&config_path, config_content).unwrap();

    // Spawn server with proxy support
    let server = TestServer::spawn_with_config(port, config_path).await.unwrap();
    let addr = server.address();

    // Barrier to synchronize start of "Good Client" after "Slow Client" has established connection
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let addr_clone = addr.clone();

    // 1. Spawn "Slowloris" client in a separate thread (blocking I/O easier here)
    thread::spawn(move || {
        let mut stream = TcpStream::connect(&addr_clone).expect("Slow client connect failed");
        
        // Send partial PROXY header
        // "PROXY TCP4 " ... wait ...
        stream.write_all(b"PROXY TCP4 192.168.0.1 192.168.0.2 12345 6667").unwrap();
        // Do NOT send \r\n yet.
        
        // Signal that we are connected and stalling
        barrier_clone.wait();

        // Sleep longer than the "Good Client" timeout
        thread::sleep(Duration::from_secs(2));

        // Finish header (too late, but keeps connection open)
        let _ = stream.write_all(b"\r\n");
    });

    // Wait for Slow client to be connected and stalling
    barrier.wait();
    // Give a tiny bit of time for the server to accept() the slow connection and enter the handler
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 2. Connect "Good" Client
    // If the server is blocking on the Slow client, this connect or handshake will hang.
    let good_client_connect = tokio::time::timeout(Duration::from_secs(1), async {
        let mut stream = tokio::net::TcpStream::connect(&addr).await?;
        
        // Send valid PROXY header immediately
        use tokio::io::AsyncWriteExt;
        stream.write_all(b"PROXY TCP4 10.0.0.1 10.0.0.2 12345 6667\r\n").await?;
        
        // Perform IRC handshake
        stream.write_all(b"NICK goodclient\r\nUSER good 0 * :Good Client\r\n").await?;

        // Expect welcome
        let mut buf = [0u8; 512];
        use tokio::io::AsyncReadExt;
        let n = stream.read(&mut buf).await?;
        let items = String::from_utf8_lossy(&buf[..n]);
        if items.contains("001 goodclient") {
            Ok::<(), anyhow::Error>(())
        } else {
            Err(anyhow::anyhow!("Did not receive welcome"))
        }
    });

    match good_client_connect.await {
        Ok(Ok(_)) => {
            println!("Good client connected successfully while SlowClient was stalling!");
        }
        Ok(Err(e)) => {
            panic!("Good client failed to register: {}", e);
        }
        Err(_) => {
            panic!("Good client timed out! Server is likely blocked by SlowClient.");
        }
    }
}
