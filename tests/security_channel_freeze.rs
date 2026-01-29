use crate::common::server::TestServer;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_channel_freeze_protection() {
    let server = TestServer::spawn(56668).await.unwrap();
    let addr = server.address();

    let channel = "#freeze";

    // 1. Connect Victim (B) - Joins and then stops reading
    let addr_b = addr.clone();
    let victim_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(&addr_b).unwrap();
        stream.write_all(b"NICK victim\r\nUSER victim 0 * :Victim\r\n").unwrap();
        // Read welcome to ensure registration
        let mut buf = [0u8; 512];
        stream.read(&mut buf).unwrap();
        stream.write_all(format!("JOIN {}\r\n", channel).as_bytes()).unwrap();
        
        // Stop reading. TCP buffer will fill up.
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    });

    // 2. Connect Sender (A) - Floods the channel
    let mut client_a = server.connect("sender").await.unwrap();
    client_a.register().await.unwrap();
    client_a.join(channel).await.unwrap();

    // 3. Connect Observer (C) - Verifies liveness
    let mut client_c = server.connect("observer").await.unwrap();
    client_c.register().await.unwrap();
    client_c.join(channel).await.unwrap();

    // Wait for joins to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Sender floods the channel to fill Victim's buffer
    println!("Sender starting flood...");
    for i in 0..5000 {
        // Send long messages to fill buffer faster
        let msg = format!("PRIVMSG {} :Flood message {} filling the buffer {}\r\n", channel, i, "x".repeat(100));
        client_a.send_raw(&msg).await.unwrap();
        
        // Yield occasionally to let server process
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    println!("Flood complete.");

    // 5. Verify Liveness
    println!("Observer checking liveness...");
    client_c.privmsg(channel, "LIVENESS_CHECK").await.unwrap();

    let check = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let msg = client_a.recv().await.unwrap();
            let raw = msg.to_string();
            if raw.contains("LIVENESS_CHECK") {
                return Ok::<(), anyhow::Error>(());
            }
        }
    }).await;

    match check {
        Ok(_) => println!("PASSED: Channel is alive despite victim blocking."),
        Err(_) => panic!("FAILED: Channel appears frozen! Sender did not receive liveness check."),
    }
}

#[tokio::test]
async fn test_mode_freeze() {
    let server = TestServer::spawn(56669).await.unwrap();
    let addr = server.address();
    let channel = "#modefreeze";

    // 1. Connect Victim (B) - Joins and then stops reading
    let addr_b = addr.clone();
    let _victim_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect(&addr_b).unwrap();
        stream.write_all(b"NICK victim\r\nUSER victim 0 * :Victim\r\n").unwrap();
        let mut buf = [0u8; 512];
        stream.read(&mut buf).unwrap();
        stream.write_all(format!("JOIN {}\r\n", channel).as_bytes()).unwrap();
        // Stop reading, block TCP buffer.
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    });

    // 2. Connect Sender (A) - Floods MODE changes
    let mut client_a = server.connect("sender").await.unwrap();
    client_a.register().await.unwrap();
    client_a.join(channel).await.unwrap();
    client_a.mode_channel_op(channel, "sender").await.unwrap(); // Op self to allow mode changes

    // 3. Connect Monitor (C) - Sends liveness check
    let mut client_c = server.connect("monitor").await.unwrap();
    client_c.register().await.unwrap();
    client_c.join(channel).await.unwrap();

    // 4. Connect Listener (D) - Verifies liveness check receipt
    let mut client_d = server.connect("listener").await.unwrap();
    client_d.register().await.unwrap();
    client_d.join(channel).await.unwrap();

    // Wait for joins
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 5. Sender floods MODE changes
    println!("Sender starting MODE flood...");
    let flood_task = tokio::spawn(async move {
        for i in 0..50 {
            let mode = if i % 2 == 0 { "+t" } else { "-t" };
            if client_a.send_raw(&format!("MODE {} {}\r\n", channel, mode)).await.is_err() {
                 break;
            }
            if i % 10 == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        client_a
    });

    // 6. Monitor triggers liveness check concurrently
    println!("Monitor sending liveness check...");
    tokio::time::sleep(Duration::from_millis(200)).await; // Wait for some flood to happen
    client_c.privmsg(channel, "LIVENESS_CHECK").await.unwrap();

    let check = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // Listener should receive the LIVENESS_CHECK from Monitor
            let msg = client_d.recv().await.unwrap();
            if msg.to_string().contains("LIVENESS_CHECK") {
                return Ok::<(), anyhow::Error>(());
            }
        }
    }).await;

    match check {
        Ok(_) => println!("PASSED: MODE flood did not freeze channel."),
        Err(_) => panic!("FAILED: Channel frozen by MODE flood! Liveness check timed out."),
    }
    
    // Cleanup
    let _ = flood_task.await;
}
