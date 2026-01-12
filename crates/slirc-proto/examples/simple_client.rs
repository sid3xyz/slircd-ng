//! Simple IRC client example
//!
//! This example demonstrates how to create a basic IRC client using slirc-proto.
//! It shows connecting to a server, authenticating, joining channels, and sending messages.

use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

use slirc_proto::{Command, Message, Transport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to an IRC server
    let stream = TcpStream::connect("irc.libera.chat:6667").await?;
    let mut transport = Transport::tcp(stream)?;

    // Send NICK and USER commands for registration
    let nick_msg = Message {
        tags: None,
        prefix: None,
        command: Command::NICK("example_bot".to_string()),
    };
    println!("→ {}", nick_msg);
    transport.write_message(&nick_msg).await?;

    let user_msg = Message {
        tags: None,
        prefix: None,
        command: Command::USER(
            "example".to_string(),
            "0".to_string(),
            "Example Bot".to_string(),
        ),
    };
    println!("→ {}", user_msg);
    transport.write_message(&user_msg).await?;

    // Wait for registration to complete
    loop {
        match timeout(Duration::from_secs(30), transport.read_message()).await {
            Ok(Ok(Some(message))) => {
                println!("← {}", message);

                match &message.command {
                    Command::Response(code, _) if code.code() == 1 => {
                        println!("✓ Registration successful!");
                        break;
                    }
                    Command::PING(server, _) => {
                        // Respond to PING
                        let pong = Message {
                            tags: None,
                            prefix: None,
                            command: Command::PONG(server.clone(), None),
                        };
                        println!("→ {}", pong);
                        transport.write_message(&pong).await?;
                    }
                    _ => {}
                }
            }
            Ok(Ok(None)) => {
                println!("Connection closed during registration");
                return Ok(());
            }
            Ok(Err(e)) => {
                eprintln!("Error during registration: {:?}", e);
                return Err(format!("Transport error: {:?}", e).into());
            }
            Err(_) => {
                eprintln!("Registration timeout");
                return Ok(());
            }
        }
    }

    // Join a channel
    let join_msg = Message {
        tags: None,
        prefix: None,
        command: Command::JOIN("#example".to_string(), None, None),
    };
    println!("→ {}", join_msg);
    transport.write_message(&join_msg).await?;

    // Send a welcome message
    let welcome_msg = Message {
        tags: None,
        prefix: None,
        command: Command::PRIVMSG(
            "#example".to_string(),
            "Hello from slirc-proto example!".to_string(),
        ),
    };
    println!("→ {}", welcome_msg);
    transport.write_message(&welcome_msg).await?;

    // Listen for messages
    println!("\n--- Listening for messages (Ctrl+C to exit) ---");

    loop {
        match timeout(Duration::from_secs(300), transport.read_message()).await {
            Ok(Ok(Some(message))) => {
                println!("← {}", message);

                match &message.command {
                    Command::PING(server, _) => {
                        // Always respond to PING
                        let pong = Message {
                            tags: None,
                            prefix: None,
                            command: Command::PONG(server.clone(), None),
                        };
                        println!("→ {}", pong);
                        transport.write_message(&pong).await?;
                    }
                    Command::PRIVMSG(target, text) => {
                        if text.contains("hello") {
                            // Respond to greetings
                            let response = Message {
                                tags: None,
                                prefix: None,
                                command: Command::PRIVMSG(
                                    target.clone(),
                                    "Hello there! 👋".to_string(),
                                ),
                            };
                            println!("→ {}", response);
                            transport.write_message(&response).await?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Ok(None)) => {
                println!("Connection closed");
                break;
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving message: {:?}", e);
                break;
            }
            Err(_) => {
                println!("No messages received in 5 minutes, keeping alive...");
            }
        }
    }

    // Send QUIT message
    let quit_msg = Message {
        tags: None,
        prefix: None,
        command: Command::QUIT(Some("Goodbye!".to_string())),
    };
    println!("→ {}", quit_msg);
    transport.write_message(&quit_msg).await?;

    Ok(())
}
