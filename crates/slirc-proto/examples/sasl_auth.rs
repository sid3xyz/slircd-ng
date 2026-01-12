//! SASL authentication example
//!
//! This example demonstrates how to use the SASL module for IRC authentication:
//! - SASL PLAIN mechanism with username/password
//! - SASL EXTERNAL mechanism for certificate authentication
//! - Proper CAP negotiation for SASL support
//! - Base64 chunking for long authentication strings

#![allow(dead_code)] // Example code - not all functions are used in main

use std::time::Duration;
use tokio::time::timeout;

use slirc_proto::{
    command::subcommands::CapSubCommand,
    sasl::{encode_external, encode_plain, SaslMechanism},
    Command, Message, Transport,
};

struct SaslClient {
    transport: Transport,
    nick: String,
    username: String,
    password: String,
    use_external: bool,
}

impl SaslClient {
    async fn new(
        server: &str,
        nick: &str,
        username: &str,
        password: &str,
        use_external: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let stream = tokio::net::TcpStream::connect(server).await?;
        let transport = if use_external {
            // For EXTERNAL, you would typically use TLS with client certificates
            // This is a simplified example - would need TLS stream setup
            Transport::tcp(stream)?
        } else {
            Transport::tcp(stream)?
        };

        Ok(SaslClient {
            transport,
            nick: nick.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            use_external,
        })
    }

    async fn connect_with_sasl(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔐 Starting SASL authentication...");

        // Step 1: Request SASL capability
        self.send_message(Command::CAP(
            None,
            CapSubCommand::LS,
            Some("302".to_string()),
            None,
        ))
        .await?;

        // Process capability negotiation
        let mut sasl_available = false;
        let mut caps_done = false;

        while !caps_done {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => {
                    match &message.command {
                        Command::CAP(_, subcommand, _, params) => {
                            match subcommand {
                                CapSubCommand::LS => {
                                    // Check if SASL is supported
                                    let caps_list = params.as_deref().unwrap_or("");
                                    if caps_list.contains("sasl") {
                                        sasl_available = true;
                                        println!("✓ Server supports SASL");

                                        // Request SASL capability
                                        self.send_message(Command::CAP(
                                            None,
                                            CapSubCommand::REQ,
                                            Some("sasl".to_string()),
                                            None,
                                        ))
                                        .await?;
                                    } else {
                                        println!("❌ Server does not support SASL");
                                        return Err("SASL not supported by server".into());
                                    }
                                }
                                CapSubCommand::ACK => {
                                    if params.as_deref().unwrap_or("").contains("sasl") {
                                        println!("✓ SASL capability acknowledged");

                                        // Begin SASL authentication
                                        if self.use_external {
                                            self.authenticate_external().await?;
                                        } else {
                                            self.authenticate_plain().await?;
                                        }

                                        caps_done = true;
                                    }
                                }
                                CapSubCommand::NAK => {
                                    println!("❌ SASL capability rejected");
                                    return Err("SASL capability rejected".into());
                                }
                                _ => {
                                    println!("← CAP {}: {:?}", subcommand, params);
                                }
                            }
                        }
                        _ => {
                            println!("← {}", message);
                        }
                    }
                }
                Ok(Ok(None)) => {
                    return Err("Connection closed during capability negotiation".into())
                }
                Ok(Err(e)) => return Err(format!("Transport error: {:?}", e).into()),
                Err(_) => return Err("Capability negotiation timeout".into()),
            }
        }

        if !sasl_available {
            return Err("SASL not available".into());
        }

        Ok(())
    }

    async fn authenticate_plain(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔑 Authenticating with SASL PLAIN...");

        // Start PLAIN authentication
        self.send_message(Command::AUTHENTICATE("PLAIN".to_string()))
            .await?;

        // Wait for authentication continuation
        loop {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => {
                    match &message.command {
                        Command::AUTHENTICATE(data) => {
                            if data == "+" {
                                // Server requests authentication data
                                println!("→ Sending PLAIN credentials...");

                                // Encode PLAIN credentials
                                let auth_string = encode_plain(&self.username, &self.password);

                                // Send in chunks if needed (400 byte limit per message)
                                let chunks = auth_string
                                    .chars()
                                    .collect::<Vec<char>>()
                                    .chunks(400)
                                    .map(|chunk| chunk.iter().collect::<String>())
                                    .collect::<Vec<String>>();

                                for chunk in &chunks {
                                    self.send_message(Command::AUTHENTICATE(chunk.clone()))
                                        .await?;
                                }

                                // Send final + if we sent all data
                                if chunks.is_empty() || chunks.last().unwrap().len() < 400 {
                                    // Data fits in chunks, we're done
                                } else {
                                    // Send continuation marker
                                    self.send_message(Command::AUTHENTICATE("+".to_string()))
                                        .await?;
                                }
                            } else {
                                println!("← AUTHENTICATE: {}", data);
                            }
                        }
                        Command::Response(response, params) => {
                            match response.code() {
                                900 => {
                                    // RPL_LOGGEDIN
                                    println!("✓ SASL authentication successful!");
                                    println!(
                                        "  Logged in as: {}",
                                        params.get(2).unwrap_or(&"unknown".to_string())
                                    );

                                    // End capability negotiation
                                    self.send_message(Command::CAP(
                                        None,
                                        CapSubCommand::END,
                                        None,
                                        None,
                                    ))
                                    .await?;

                                    return Ok(());
                                }
                                901 => {
                                    // RPL_LOGGEDOUT
                                    println!("ℹ️  SASL logout acknowledged");
                                }
                                902 => {
                                    // ERR_NICKLOCKED
                                    println!("❌ SASL failed: Nick locked");
                                    return Err("Nick locked".into());
                                }
                                903 => {
                                    // RPL_SASLSUCCESS
                                    println!("✓ SASL authentication completed successfully!");

                                    // End capability negotiation
                                    self.send_message(Command::CAP(
                                        None,
                                        CapSubCommand::END,
                                        None,
                                        None,
                                    ))
                                    .await?;

                                    return Ok(());
                                }
                                904 => {
                                    // ERR_SASLFAIL
                                    println!("❌ SASL authentication failed");
                                    return Err("SASL authentication failed".into());
                                }
                                905 => {
                                    // ERR_SASLTOOLONG
                                    println!("❌ SASL authentication string too long");
                                    return Err("SASL auth string too long".into());
                                }
                                906 => {
                                    // ERR_SASLABORTED
                                    println!("❌ SASL authentication aborted");
                                    return Err("SASL authentication aborted".into());
                                }
                                907 => {
                                    // ERR_SASLALREADY
                                    println!("❌ Already authenticated via SASL");
                                    return Err("Already authenticated".into());
                                }
                                908 => {
                                    // RPL_SASLMECHS
                                    let default_mechanisms = "none".to_string();
                                    let mechanisms = params.get(1).unwrap_or(&default_mechanisms);
                                    println!("📋 Available SASL mechanisms: {}", mechanisms);
                                }
                                _ => {
                                    println!("← Response {}: {:?}", response.code(), params);
                                }
                            }
                        }
                        _ => {
                            println!("← {}", message);
                        }
                    }
                }
                Ok(Ok(None)) => return Err("Connection closed during authentication".into()),
                Ok(Err(e)) => return Err(format!("Transport error: {:?}", e).into()),
                Err(_) => return Err("Authentication timeout".into()),
            }
        }
    }

    async fn authenticate_external(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔑 Authenticating with SASL EXTERNAL...");

        // Start EXTERNAL authentication
        self.send_message(Command::AUTHENTICATE("EXTERNAL".to_string()))
            .await?;

        // Wait for authentication continuation
        loop {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => {
                    match &message.command {
                        Command::AUTHENTICATE(data) => {
                            if data == "+" {
                                // Server requests authentication data
                                println!("→ Sending EXTERNAL credentials...");

                                // EXTERNAL typically uses the username or can be empty
                                let auth_string = encode_external(Some(&self.username));
                                self.send_message(Command::AUTHENTICATE(auth_string))
                                    .await?;
                            } else {
                                println!("← AUTHENTICATE: {}", data);
                            }
                        }
                        Command::Response(response, _) => {
                            match response.code() {
                                900 | 903 => {
                                    println!("✓ SASL EXTERNAL authentication successful!");

                                    // End capability negotiation
                                    self.send_message(Command::CAP(
                                        None,
                                        CapSubCommand::END,
                                        None,
                                        None,
                                    ))
                                    .await?;

                                    return Ok(());
                                }
                                904 => {
                                    println!("❌ SASL EXTERNAL authentication failed");
                                    println!("  (Make sure you have a valid client certificate)");
                                    return Err("SASL EXTERNAL failed".into());
                                }
                                _ => {
                                    // Handle other SASL responses same as PLAIN
                                }
                            }
                        }
                        _ => {
                            println!("← {}", message);
                        }
                    }
                }
                Ok(Ok(None)) => return Err("Connection closed during authentication".into()),
                Ok(Err(e)) => return Err(format!("Transport error: {:?}", e).into()),
                Err(_) => return Err("Authentication timeout".into()),
            }
        }
    }

    async fn complete_registration(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📝 Completing IRC registration...");

        // Send NICK and USER after successful SASL auth
        self.send_message(Command::NICK(self.nick.clone())).await?;
        self.send_message(Command::USER(
            self.username.clone(),
            "0".to_string(),
            "SASL Example Client".to_string(),
        ))
        .await?;

        // Wait for welcome message
        loop {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => match &message.command {
                    Command::Response(response, _) if response.code() == 1 => {
                        println!("🎉 Successfully connected and authenticated!");
                        return Ok(());
                    }
                    Command::PING(server, _) => {
                        self.send_message(Command::PONG(server.clone(), None))
                            .await?;
                    }
                    _ => {
                        println!("← {}", message);
                    }
                },
                Ok(Ok(None)) => return Err("Connection closed during registration".into()),
                Ok(Err(e)) => return Err(format!("Transport error: {:?}", e).into()),
                Err(_) => return Err("Registration timeout".into()),
            }
        }
    }

    async fn send_message(&mut self, command: Command) -> Result<(), Box<dyn std::error::Error>> {
        let message = Message {
            tags: None,
            prefix: None,
            command,
        };
        self.transport.write_message(&message).await?;
        Ok(())
    }
}

fn demonstrate_mechanisms() {
    println!("\n🔍 SASL Mechanism Demonstration:\n");

    // Demonstrate PLAIN encoding
    println!("PLAIN mechanism:");
    let plain_encoded = encode_plain("testuser", "testpass");
    println!(
        "  encode_plain(\"testuser\", \"testpass\") = \"{}\"",
        plain_encoded
    );

    // Show what the base64 decodes to (for educational purposes)
    use base64::Engine;
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&plain_encoded) {
        let decoded_str = String::from_utf8_lossy(&decoded);
        println!(
            "  Decoded format: {:?} (authzid\\0username\\0password)",
            decoded_str
        );
    }

    // Demonstrate EXTERNAL encoding
    println!("\nEXTERNAL mechanism:");
    let external_encoded = encode_external(Some("testuser"));
    println!(
        "  encode_external(Some(\"testuser\")) = \"{}\"",
        external_encoded
    );

    let external_empty = encode_external(None);
    println!("  encode_external(None) = \"{}\"", external_empty);

    // Show supported mechanisms
    println!("\nSupported mechanisms:");
    println!(
        "  {:?} - Username/password authentication",
        SaslMechanism::Plain
    );
    println!(
        "  {:?} - Client certificate authentication",
        SaslMechanism::External
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Optional: Initialize tracing for debugging
    // tracing_subscriber::init();

    // Demonstrate SASL encoding without connecting
    println!("=== SASL Authentication Example ===\n");

    demonstrate_mechanisms();

    println!("\n=== Live Connection Example ===");
    println!("Note: This example requires valid credentials and a SASL-enabled server");
    println!("Update the connection details below to test with a real server:\n");

    // Example with PLAIN authentication (commented out to avoid connection attempts)
    /*
    let mut client = SaslClient::new(
        "irc.libera.chat:6697",  // Use TLS port for secure authentication
        "sasl_test_nick",
        "your_username",
        "your_password",
        false, // Use PLAIN, not EXTERNAL
    ).await?;

    client.connect_with_sasl().await?;
    client.complete_registration().await?;

    // Join a channel and send a message
    client.send_message(Command::JOIN("#test".to_string(), None)).await?;
    client.send_message(Command::PRIVMSG(
        "#test".to_string(),
        "Hello from SASL authenticated client!".to_string()
    )).await?;

    // Send quit message
    client.send_message(Command::QUIT(Some("SASL demo complete".to_string()))).await?;
    */

    println!("✓ SASL demonstration complete!");
    println!("Uncomment the connection code above to test with real credentials.");

    Ok(())
}
