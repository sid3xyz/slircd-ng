//! Test IRC client.
//!
//! Provides an IRC client for integration testing that can send commands
//! and assert on received responses.

use slirc_proto::{Command, Message};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::timeout;

/// A test IRC client.
pub struct TestClient {
    reader: BufReader<OwnedReadHalf>,
    writer: BufWriter<OwnedWriteHalf>,
    nick: String,
}

impl TestClient {
    /// Connect to a test server.
    pub async fn connect(address: &str, nick: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(address).await?;

        // Split stream for reading and writing
        let (read_half, write_half) = stream.into_split();
        let reader = BufReader::new(read_half);
        let writer = BufWriter::new(write_half);

        Ok(Self {
            reader,
            writer,
            nick: nick.to_string(),
        })
    }

    /// Send a raw IRC message.
    pub async fn send_raw(&mut self, line: &str) -> anyhow::Result<()> {
        self.writer.write_all(line.as_bytes()).await?;
        if !line.ends_with("\r\n") {
            self.writer.write_all(b"\r\n").await?;
        }
        self.writer.flush().await?;
        Ok(())
    }

    /// Send an IRC command.
    pub async fn send(&mut self, cmd: Command) -> anyhow::Result<()> {
        let msg = Message::from(cmd);
        self.send_raw(&msg.to_string()).await
    }

    /// Receive a single message from the server.
    pub async fn recv(&mut self) -> anyhow::Result<Message> {
        self.recv_timeout(Duration::from_secs(5)).await
    }

    /// Receive a message with a timeout.
    pub async fn recv_timeout(&mut self, dur: Duration) -> anyhow::Result<Message> {
        let mut line = String::new();
        timeout(dur, self.reader.read_line(&mut line)).await??;

        // Parse the line directly into owned Message using Message::parse
        // (slirc_proto provides this for owned parsing)
        line.trim_end()
            .parse::<Message>()
            .map_err(|e| anyhow::anyhow!("Parse error: {}", e))
    }

    /// Receive multiple messages until the given predicate returns true.
    pub async fn recv_until<F>(&mut self, mut predicate: F) -> anyhow::Result<Vec<Message>>
    where
        F: FnMut(&Message) -> bool,
    {
        let mut messages = Vec::new();
        loop {
            let msg = self.recv().await?;
            let done = predicate(&msg);
            messages.push(msg);
            if done {
                break;
            }
        }
        Ok(messages)
    }

    /// Register with the server (NICK + USER).
    pub async fn register(&mut self) -> anyhow::Result<()> {
        self.send(Command::NICK(self.nick.clone())).await?;
        self.send(Command::USER(
            self.nick.clone(),
            "0".to_string(),
            format!("Test User {}", self.nick),
        ))
        .await?;

        // Wait for RPL_WELCOME (001)
        let messages = self
            .recv_until(
                |msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 1),
            )
            .await?;

        if messages
            .iter()
            .any(|msg| matches!(&msg.command, Command::Response(resp, _) if resp.code() == 1))
        {
            Ok(())
        } else {
            anyhow::bail!("Registration failed: no RPL_WELCOME received")
        }
    }

    /// Join a channel.
    #[allow(dead_code)]
    pub async fn join(&mut self, channel: &str) -> anyhow::Result<()> {
        self.send(Command::JOIN(channel.to_string(), None, None))
            .await?;
        Ok(())
    }

    /// Send a PRIVMSG.
    #[allow(dead_code)]
    pub async fn privmsg(&mut self, target: &str, text: &str) -> anyhow::Result<()> {
        self.send(Command::PRIVMSG(target.to_string(), text.to_string()))
            .await?;
        Ok(())
    }

    /// Send QUIT and close the connection.
    pub async fn quit(&mut self, reason: Option<String>) -> anyhow::Result<()> {
        self.send(Command::QUIT(reason)).await?;
        Ok(())
    }
}
