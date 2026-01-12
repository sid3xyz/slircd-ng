//! IRC bot example with advanced message handling
//!
//! This example shows how to build a more sophisticated IRC bot that:
//! - Handles PING/PONG automatically
//! - Responds to commands in channels and private messages
//! - Uses CTCP for VERSION and PING responses
//! - Demonstrates proper error handling and reconnection

// use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};

use slirc_proto::{
    ctcp::{Ctcp, CtcpKind},
    Command, Message, Transport,
};

struct Bot {
    nick: String,
    channels: Vec<String>,
    transport: Transport,
    command_prefix: String,
    stats: BotStats,
}

#[derive(Default)]
struct BotStats {
    messages_received: u64,
    commands_processed: u64,
    uptime_start: Option<SystemTime>,
}

impl Bot {
    async fn new(
        server: &str,
        nick: &str,
        channels: Vec<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let stream = tokio::net::TcpStream::connect(server).await?;
        let transport = Transport::tcp(stream)?;

        Ok(Bot {
            nick: nick.to_string(),
            channels,
            transport,
            command_prefix: "!".to_string(),
            stats: BotStats::default(),
        })
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Send registration
        self.send_message(Command::NICK(self.nick.clone())).await?;
        self.send_message(Command::USER(
            "bot".to_string(),
            "0".to_string(),
            "slirc-proto Example Bot".to_string(),
        ))
        .await?;

        // Wait for welcome message
        loop {
            match timeout(Duration::from_secs(30), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => match &message.command {
                    Command::Response(response, _) if response.code() == 1 => {
                        println!("✓ Connected to server!");
                        self.stats.uptime_start = Some(SystemTime::now());
                        break;
                    }
                    Command::PING(server, _) => {
                        self.handle_ping(server).await?;
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

        // Join channels
        let channels = self.channels.clone();
        for channel in channels {
            self.send_message(Command::JOIN(channel.clone(), None, None))
                .await?;
            println!("→ Joining {}", channel);
        }

        Ok(())
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "🤖 Bot is running! Use {}help for commands",
            self.command_prefix
        );

        loop {
            match timeout(Duration::from_secs(300), self.transport.read_message()).await {
                Ok(Ok(Some(message))) => {
                    self.stats.messages_received += 1;
                    self.handle_message(message).await?;
                }
                Ok(Ok(None)) => {
                    println!("Connection closed, attempting to reconnect...");
                    return self.reconnect().await;
                }
                Ok(Err(e)) => {
                    eprintln!("Message error: {:?}", e);
                    sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // Send keepalive PING to server
                    self.send_message(Command::PING("keepalive".to_string(), None))
                        .await?;
                }
            }
        }
    }

    async fn handle_message(&mut self, message: Message) -> Result<(), Box<dyn std::error::Error>> {
        match &message.command {
            Command::PING(server, _) => {
                self.handle_ping(server).await?;
            }
            Command::PRIVMSG(target, text) => {
                // Check for CTCP messages
                if let Some(ctcp) = Ctcp::parse(text) {
                    self.handle_ctcp(&message, target, &ctcp).await?;
                } else if text.starts_with(&self.command_prefix) {
                    // Handle bot commands
                    self.handle_command(&message, target, text).await?;
                } else {
                    // Log regular messages
                    if let Some(prefix) = &message.prefix {
                        println!("[{}] <{}> {}", target, prefix.nick().unwrap_or("?"), text);
                    }
                }
            }
            Command::JOIN(channel, _, _) => {
                if let Some(prefix) = &message.prefix {
                    if prefix.nick() == Some(&self.nick) {
                        println!("✓ Joined {}", channel);
                    } else {
                        println!("[{}] → {} joined", channel, prefix.nick().unwrap_or("?"));
                    }
                }
            }
            Command::PART(channel, reason) => {
                if let Some(prefix) = &message.prefix {
                    let reason_str = reason.as_deref().unwrap_or("");
                    println!(
                        "[{}] ← {} left ({})",
                        channel,
                        prefix.nick().unwrap_or("?"),
                        reason_str
                    );
                }
            }
            Command::QUIT(reason) => {
                if let Some(prefix) = &message.prefix {
                    let reason_str = reason.as_deref().unwrap_or("");
                    println!("← {} quit ({})", prefix.nick().unwrap_or("?"), reason_str);
                }
            }
            _ => {
                // Log other messages in debug mode
                println!("← {}", message);
            }
        }

        Ok(())
    }

    async fn handle_ping(&mut self, server: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.send_message(Command::PONG(server.to_string(), None))
            .await?;
        Ok(())
    }

    async fn handle_ctcp(
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

        match &ctcp.kind {
            CtcpKind::Version => {
                println!("[CTCP] {} requested VERSION", sender);
                let response = Ctcp {
                    kind: CtcpKind::Version,
                    params: Some("slirc-proto-bot v0.1.0"),
                };
                let reply_target = if target.starts_with('#') {
                    sender
                } else {
                    target
                };
                self.send_ctcp_reply(reply_target, &response).await?;
            }
            CtcpKind::Ping => {
                println!("[CTCP] {} sent PING", sender);
                let response = Ctcp {
                    kind: CtcpKind::Ping,
                    params: ctcp.params,
                };
                let reply_target = if target.starts_with('#') {
                    sender
                } else {
                    target
                };
                self.send_ctcp_reply(reply_target, &response).await?;
            }
            CtcpKind::Action => {
                let action = ctcp.params.unwrap_or("");
                println!("[{}] * {} {}", target, sender, action);
            }
            _ => {
                println!("[CTCP] {} sent {}: {:?}", sender, ctcp.kind, ctcp.params);
            }
        }

        Ok(())
    }

    async fn handle_command(
        &mut self,
        message: &Message,
        target: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sender = message
            .prefix
            .as_ref()
            .and_then(|p| p.nick())
            .unwrap_or("unknown");

        let command_text = &text[self.command_prefix.len()..];
        let parts: Vec<&str> = command_text.split_whitespace().collect();

        if parts.is_empty() {
            return Ok(());
        }

        let command = parts[0].to_lowercase();
        let reply_target = if target.starts_with('#') {
            target
        } else {
            sender
        };

        self.stats.commands_processed += 1;

        match command.as_str() {
            "help" => {
                let help_text = format!(
                    "Available commands: {}help, {}stats, {}ping, {}echo <text>, {}time",
                    self.command_prefix,
                    self.command_prefix,
                    self.command_prefix,
                    self.command_prefix,
                    self.command_prefix
                );
                self.send_reply(reply_target, &help_text).await?;
            }
            "stats" => {
                let uptime = self
                    .stats
                    .uptime_start
                    .map(|start| {
                        SystemTime::now()
                            .duration_since(start)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0);
                let stats_text = format!(
                    "📊 Messages: {}, Commands: {}, Uptime: {}s",
                    self.stats.messages_received, self.stats.commands_processed, uptime
                );
                self.send_reply(reply_target, &stats_text).await?;
            }
            "ping" => {
                self.send_reply(reply_target, "🏓 Pong!").await?;
            }
            "time" => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.send_reply(reply_target, &format!("🕐 Unix timestamp: {}", now))
                    .await?;
            }
            "echo" => {
                if parts.len() > 1 {
                    let echo_text = parts[1..].join(" ");
                    self.send_reply(reply_target, &format!("📢 {}", echo_text))
                        .await?;
                } else {
                    self.send_reply(reply_target, "Usage: !echo <text>").await?;
                }
            }
            _ => {
                self.send_reply(
                    reply_target,
                    &format!(
                        "❓ Unknown command '{}'. Use {}help for available commands.",
                        command, self.command_prefix
                    ),
                )
                .await?;
            }
        }

        println!(
            "[COMMAND] {} used {}{}",
            sender, self.command_prefix, command
        );
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

    async fn send_reply(
        &mut self,
        target: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.send_message(Command::PRIVMSG(target.to_string(), text.to_string()))
            .await
    }

    async fn send_ctcp_reply(
        &mut self,
        target: &str,
        ctcp: &Ctcp<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ctcp_text = ctcp.to_string();
        self.send_message(Command::NOTICE(target.to_string(), ctcp_text))
            .await
    }

    async fn reconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔄 Reconnecting in 5 seconds...");
        sleep(Duration::from_secs(5)).await;

        // Try to reconnect
        match tokio::net::TcpStream::connect("irc.libera.chat:6667").await {
            Ok(stream) => {
                self.transport = Transport::tcp(stream)?;
                self.connect().await?;
                println!("✓ Reconnected successfully!");
                Ok(())
            }
            Err(e) => {
                eprintln!("❌ Reconnection failed: {}", e);
                Err(e.into())
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Optional: Initialize tracing for debugging
    // tracing_subscriber::init();

    let mut bot = Bot::new(
        "irc.libera.chat:6667",
        "slirc_bot",
        vec!["#slirc-test".to_string()],
    )
    .await?;

    bot.connect().await?;
    bot.run().await
}
