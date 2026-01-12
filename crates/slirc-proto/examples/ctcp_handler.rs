//! CTCP message handling example
//!
//! This example demonstrates how to use the CTCP module to:
//! - Parse incoming CTCP messages
//! - Generate CTCP responses  
//! - Handle common CTCP commands like ACTION, VERSION, PING, TIME
//! - Implement proper CTCP reply handling

use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

use slirc_proto::{
    ctcp::{Ctcp, CtcpKind},
    Command, Message, Transport,
};

struct CtcpHandler {
    transport: Transport,
    nick: String,
}

impl CtcpHandler {
    async fn new(server: &str, nick: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let stream = tokio::net::TcpStream::connect(server).await?;
        let transport = Transport::tcp(stream)?;
        Ok(CtcpHandler {
            transport,
            nick: nick.to_string(),
        })
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Register with server
        self.send_message(Command::NICK(self.nick.clone())).await?;
        self.send_message(Command::USER(
            "ctcp_demo".to_string(),
            "0".to_string(),
            "CTCP Demo Client".to_string(),
        ))
        .await?;

        // Wait for welcome
        loop {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => match &message.command {
                    Command::Response(response, _) if response.code() == 1 => {
                        println!("✓ Connected and registered!");
                        break;
                    }
                    Command::PING(server, _) => {
                        self.handle_ping(server).await?;
                    }
                    _ => {
                        println!("← {}", message);
                    }
                },
                Ok(Ok(None)) => return Err("Connection closed".into()),
                Ok(Err(e)) => return Err(format!("Transport error: {:?}", e).into()),
                Err(_) => return Err("Connection timeout".into()),
            }
        }

        // Join test channel
        self.send_message(Command::JOIN("#ctcp-test".to_string(), None, None))
            .await?;
        println!("→ Joined #ctcp-test for CTCP demonstrations");

        Ok(())
    }

    async fn demonstrate_ctcp(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n🔧 CTCP Demonstration Starting...\n");

        // Demonstrate creating various CTCP messages
        self.demonstrate_ctcp_creation().await?;

        // Send some CTCP messages to the channel for demonstration
        self.send_demo_messages().await?;

        // Listen for CTCP messages and handle them
        self.listen_for_ctcp().await?;

        Ok(())
    }

    async fn demonstrate_ctcp_creation(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📝 Creating CTCP messages:");

        // ACTION message
        let action = Ctcp {
            kind: CtcpKind::Action,
            params: Some("is demonstrating CTCP functionality"),
        };
        println!("ACTION: {}", action);

        // VERSION request
        let version_request = Ctcp {
            kind: CtcpKind::Version,
            params: None,
        };
        println!("VERSION request: {}", version_request);

        // VERSION response
        let version_response = Ctcp {
            kind: CtcpKind::Version,
            params: Some("slirc-proto v0.2.0 / Rust"),
        };
        println!("VERSION response: {}", version_response);

        // PING with timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();
        let ping = Ctcp {
            kind: CtcpKind::Ping,
            params: Some(&timestamp),
        };
        println!("PING: {}", ping);

        // TIME request
        let time_request = Ctcp {
            kind: CtcpKind::Time,
            params: None,
        };
        println!("TIME request: {}", time_request);

        // Custom CTCP
        let custom = Ctcp {
            kind: CtcpKind::parse("FINGER"),
            params: Some("CTCP demonstration user"),
        };
        println!("Custom CTCP: {}", custom);

        println!();
        Ok(())
    }

    async fn send_demo_messages(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🚀 Sending demonstration messages:\n");

        // Send an ACTION to the channel
        let action = Ctcp {
            kind: CtcpKind::Action,
            params: Some("waves hello to everyone! 👋"),
        };
        self.send_ctcp_message("#ctcp-test", &action).await?;
        println!("→ Sent ACTION to #ctcp-test");

        // Send informational message
        self.send_message(Command::PRIVMSG(
            "#ctcp-test".to_string(),
            "This client demonstrates CTCP parsing. Try sending me VERSION, PING, or TIME requests!".to_string(),
        )).await?;

        Ok(())
    }

    async fn listen_for_ctcp(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("👂 Listening for CTCP messages (send me CTCP requests!):");
        println!("   Examples: /ctcp {} VERSION", self.nick);
        println!("            /ctcp {} PING", self.nick);
        println!("            /ctcp {} TIME", self.nick);
        println!("Press Ctrl+C to exit\n");

        loop {
            match timeout(Duration::from_secs(300), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => {
                    self.handle_message(message).await?;
                }
                Ok(Ok(None)) => {
                    println!("Connection closed");
                    break;
                }
                Ok(Err(e)) => {
                    eprintln!("Error receiving message: {:?}", e);
                }
                Err(_) => {
                    // Send keepalive
                    self.send_message(Command::PING("keepalive".to_string(), None))
                        .await?;
                    println!("⏱️  Sent keepalive (no activity for 5 minutes)");
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<(), Box<dyn std::error::Error>> {
        match &message.command {
            Command::PING(server, _) => {
                self.handle_ping(server).await?;
            }
            Command::PRIVMSG(target, text) => {
                let sender = message
                    .prefix
                    .as_ref()
                    .and_then(|p| p.nick())
                    .unwrap_or("unknown");

                // Try to parse as CTCP
                match Ctcp::parse(text) {
                    Some(ctcp) => {
                        self.handle_ctcp_message(&message, target, &ctcp).await?;
                    }
                    None => {
                        // Regular message
                        println!("[{}] <{}> {}", target, sender, text);
                    }
                }
            }
            Command::NOTICE(target, text) => {
                let sender = message
                    .prefix
                    .as_ref()
                    .and_then(|p| p.nick())
                    .unwrap_or("unknown");

                // CTCP replies are often sent as NOTICE
                match Ctcp::parse(text) {
                    Some(ctcp) => {
                        println!(
                            "📨 CTCP Reply from {}: {} = {:?}",
                            sender, ctcp.kind, ctcp.params
                        );
                    }
                    None => {
                        println!("[NOTICE {}] -{}- {}", target, sender, text);
                    }
                }
            }
            _ => {
                println!("← {}", message);
            }
        }

        Ok(())
    }

    async fn handle_ctcp_message(
        &mut self,
        message: &Message,
        target: &str,
        ctcp: &Ctcp<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sender = message
            .prefix
            .as_ref()
            .and_then(|p| p.nick())
            .unwrap_or("unknown");

        // Determine reply target (sender for channel messages, original target for private)
        let reply_target = if target.starts_with('#') {
            sender
        } else {
            target
        };

        println!("🎯 CTCP from {}: {} {:?}", sender, ctcp.kind, ctcp.params);

        match &ctcp.kind {
            CtcpKind::Version => {
                // Respond with version information
                let response = Ctcp {
                    kind: CtcpKind::Version,
                    params: Some("slirc-proto CTCP Demo v0.2.0 / Rust 1.70+ / Linux"),
                };
                self.send_ctcp_reply(reply_target, &response).await?;
                println!("   ✅ Sent VERSION reply");
            }
            CtcpKind::Ping => {
                // Echo back the PING with same parameters
                let response = Ctcp {
                    kind: CtcpKind::Ping,
                    params: ctcp.params,
                };
                self.send_ctcp_reply(reply_target, &response).await?;
                println!("   🏓 Sent PING reply");
            }
            CtcpKind::Time => {
                // Respond with current time
                let now = chrono::Utc::now()
                    .format("%a %b %e %H:%M:%S %Y %Z")
                    .to_string();
                let response = Ctcp {
                    kind: CtcpKind::Time,
                    params: Some(&now),
                };
                self.send_ctcp_reply(reply_target, &response).await?;
                println!("   🕐 Sent TIME reply");
            }
            CtcpKind::Action => {
                // Display action
                let action_text = ctcp.params.unwrap_or("");
                println!("   🎭 ACTION: * {} {}", sender, action_text);
            }
            CtcpKind::Finger => {
                // Respond to FINGER request
                let response = Ctcp {
                    kind: CtcpKind::Finger,
                    params: Some("CTCP Demo User (slirc-proto example)"),
                };
                self.send_ctcp_reply(reply_target, &response).await?;
                println!("   👆 Sent FINGER reply");
            }
            CtcpKind::Userinfo => {
                // Respond to USERINFO request
                let response = Ctcp {
                    kind: CtcpKind::Userinfo,
                    params: Some("Demonstrating CTCP functionality with slirc-proto"),
                };
                self.send_ctcp_reply(reply_target, &response).await?;
                println!("   ℹ️  Sent USERINFO reply");
            }
            CtcpKind::Unknown(command) => {
                println!("   ❓ Unknown CTCP command: {}", command);
                // Could send an error reply or just ignore
            }
            _ => {
                println!("   🔍 Other CTCP: {} {:?}", ctcp.kind, ctcp.params);
            }
        }

        Ok(())
    }

    async fn handle_ping(&mut self, server: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.send_message(Command::PONG(server.to_string(), None))
            .await?;
        Ok(())
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

    async fn send_ctcp_message(
        &mut self,
        target: &str,
        ctcp: &Ctcp<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ctcp_text = ctcp.to_string();
        self.send_message(Command::PRIVMSG(target.to_string(), ctcp_text))
            .await
    }

    async fn send_ctcp_reply(
        &mut self,
        target: &str,
        ctcp: &Ctcp<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ctcp_text = ctcp.to_string();
        // CTCP replies are typically sent as NOTICE messages
        self.send_message(Command::NOTICE(target.to_string(), ctcp_text))
            .await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Add chrono dependency for time formatting
    // Optional: Initialize tracing for debugging
    // tracing_subscriber::init();

    let mut handler = CtcpHandler::new("irc.libera.chat:6667", "ctcp_demo").await?;

    handler.connect().await?;
    handler.demonstrate_ctcp().await?;

    // Send quit message
    handler
        .send_message(Command::QUIT(Some(
            "CTCP demonstration complete!".to_string(),
        )))
        .await?;

    Ok(())
}
