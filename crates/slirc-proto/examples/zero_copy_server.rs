//! Zero-copy transport example for high-throughput servers
//!
//! This example demonstrates how to use `ZeroCopyTransport` for server-side
//! message processing with minimal allocations. This is ideal for:
//! - IRC servers handling many concurrent connections
//! - High-throughput bots or bridges
//! - Performance-critical message routing
//!
//! The key insight is that `ZeroCopyTransport` yields `MessageRef<'_>` which
//! borrows directly from the internal buffer, avoiding heap allocations in
//! the hot loop.

use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

use slirc_proto::{Message, Transport, ZeroCopyTransportEnum};

/// Simulates a high-performance IRC message processor
struct MessageProcessor {
    messages_processed: u64,
}

impl MessageProcessor {
    fn new() -> Self {
        Self {
            messages_processed: 0,
        }
    }

    /// Process a message without allocating - just inspect and route
    fn process_message_ref(&mut self, msg: &slirc_proto::MessageRef<'_>) {
        self.messages_processed += 1;

        // Zero-copy access to command name
        let cmd_name = msg.command.name;

        // Zero-copy access to tags (if present)
        if let Some(tags_str) = msg.tags {
            // tags is a raw &str - you can parse it further if needed
            // For simple checks:
            if tags_str.contains("time=") {
                // Has server-time tag
            }
        }

        // Zero-copy access to prefix
        if let Some(prefix) = &msg.prefix {
            // prefix.nick is Option<&str> - no allocation
            let _nick = prefix.nick;
        }

        // Route based on command - no allocation needed for comparison
        match cmd_name {
            "PING" => {
                // In a real server, you'd respond with PONG here
            }
            "PRIVMSG" | "NOTICE" => {
                // Access args without allocation
                if msg.command.args.len() >= 2 {
                    let _target = msg.command.args[0];
                    let _text = msg.command.args[1];
                    // Route to appropriate channel/user handler
                }
            }
            "JOIN" | "PART" | "QUIT" => {
                // Membership changes - update internal state
            }
            _ => {
                // Other commands
            }
        }
    }

    fn stats(&self) -> u64 {
        self.messages_processed
    }
}

/// Example: Accept a connection and process messages with zero-copy transport
async fn handle_connection(
    transport: Transport,
    processor: &mut MessageProcessor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert to zero-copy transport after any initial handshake
    let mut zc: ZeroCopyTransportEnum = transport.try_into().unwrap();

    println!("→ Upgraded to zero-copy transport");

    // Hot loop - no allocations per message!
    loop {
        match timeout(Duration::from_secs(300), zc.next()).await {
            Ok(Some(result)) => {
                match result {
                    Ok(msg_ref) => {
                        // Process without allocating
                        processor.process_message_ref(&msg_ref);

                        // For logging, use Debug since MessageRef doesn't impl Display
                        println!(
                            "← {} {}",
                            msg_ref.command.name,
                            msg_ref.command.args.join(" ")
                        );

                        // If you need to keep the message, you can convert to owned:
                        // let owned: Message = msg_ref.to_owned();
                    }
                    Err(e) => {
                        eprintln!("Parse error: {:?}", e);
                    }
                }
            }
            Ok(None) => {
                println!("Connection closed");
                break;
            }
            Err(_) => {
                // Timeout - connection idle
                println!("Connection idle, closing");
                break;
            }
        }
    }

    println!("Processed {} messages (zero-copy)", processor.stats());

    Ok(())
}

/// Demonstrates the difference between owned and zero-copy parsing
fn compare_parsing_approaches() {
    let raw =
        "@time=2023-01-01T12:00:00Z;msgid=abc123 :nick!user@host PRIVMSG #channel :Hello, world!";

    // Approach 1: Owned parsing (allocates)
    let owned: Message = raw.parse().unwrap();
    println!("Owned message command: {:?}", owned.command);

    // Approach 2: Zero-copy parsing (borrows from input)
    let borrowed = slirc_proto::MessageRef::parse(raw).unwrap();
    println!("Borrowed command name: {}", borrowed.command.name);
    println!("Borrowed args: {:?}", borrowed.command.args);

    // The borrowed version doesn't allocate - it just points into `raw`
    // This is what ZeroCopyTransport uses internally
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== Zero-Copy Transport Example ===\n");

    // Show the parsing comparison
    println!("--- Parsing Comparison ---");
    compare_parsing_approaches();
    println!();

    // Start a simple server that accepts one connection
    println!("--- Server Mode ---");
    println!("Starting server on 127.0.0.1:6667...");
    println!("Connect with: nc localhost 6667");
    println!("Then type IRC messages like: PING :test");
    println!("Or: PRIVMSG #test :Hello world");
    println!();

    let listener = TcpListener::bind("127.0.0.1:6667").await?;
    let mut processor = MessageProcessor::new();

    // Accept one connection for demo purposes
    let (stream, addr) = listener.accept().await?;
    println!("Accepted connection from {}", addr);

    let transport = Transport::tcp(stream)?;
    handle_connection(transport, &mut processor).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_message_ref_parsing() {
        let raw = ":server PRIVMSG #test :Hello";
        let msg = slirc_proto::MessageRef::parse(raw).unwrap();
        assert_eq!(msg.command.name, "PRIVMSG");
        assert_eq!(msg.command.args[0], "#test");
        assert_eq!(msg.command.args[1], "Hello");
    }
}
