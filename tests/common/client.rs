//! Test IRC client.
//!
//! Provides an IRC client for integration testing that can send commands
//! and assert on received responses.

use slirc_proto::{Command, Message};
use std::io::{BufReader as StdBufReader, Cursor};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader as TokioBufReader,
    BufWriter as TokioBufWriter,
};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use rustls_pemfile::{certs, pkcs8_private_keys};

use super::tls::TlsClientConfig;

/// A test IRC client.
pub struct TestClient {
    reader: TokioBufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: TokioBufWriter<Box<dyn AsyncWrite + Unpin + Send>>,
    #[allow(dead_code)]
    nick: String,
}

impl TestClient {
    /// Connect to a test server.
    pub async fn connect(address: &str, nick: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(address).await?;

        // Split stream for reading and writing
        let (read_half, write_half) = stream.into_split();
        let reader: Box<dyn AsyncRead + Unpin + Send> = Box::new(read_half);
        let writer: Box<dyn AsyncWrite + Unpin + Send> = Box::new(write_half);
        let reader = TokioBufReader::new(reader);
        let writer = TokioBufWriter::new(writer);

        Ok(Self {
            reader,
            writer,
            nick: nick.to_string(),
        })
    }

    /// Connect to a test server over TLS.
    pub async fn connect_tls(
        address: &str,
        nick: &str,
        tls: TlsClientConfig,
    ) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(address).await?;
        let tls_stream = connect_tls_stream(stream, &tls).await?;

        let (read_half, write_half) = tokio::io::split(tls_stream);
        let reader: Box<dyn AsyncRead + Unpin + Send> = Box::new(read_half);
        let writer: Box<dyn AsyncWrite + Unpin + Send> = Box::new(write_half);
        let reader = TokioBufReader::new(reader);
        let writer = TokioBufWriter::new(writer);

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
        self.recv_timeout(Duration::from_secs(15)).await
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn join(&mut self, channel: &str) -> anyhow::Result<()> {
        self.send(Command::JOIN(channel.to_string(), None, None))
            .await?;
        Ok(())
    }

    /// Send a PRIVMSG.
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn privmsg(&mut self, target: &str, text: &str) -> anyhow::Result<()> {
        self.send(Command::PRIVMSG(target.to_string(), text.to_string()))
            .await?;
        Ok(())
    }

    /// Send QUIT and close the connection.
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn quit(&mut self, reason: Option<String>) -> anyhow::Result<()> {
        self.send(Command::QUIT(reason)).await?;
        Ok(())
    }

    /// Part a channel.
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn part(&mut self, channel: &str, reason: Option<&str>) -> anyhow::Result<()> {
        self.send(Command::PART(
            channel.to_string(),
            reason.map(|r| r.to_string()),
        ))
        .await?;
        Ok(())
    }

    /// Set a channel topic.
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn topic(&mut self, channel: &str, topic: &str) -> anyhow::Result<()> {
        self.send(Command::TOPIC(channel.to_string(), Some(topic.to_string())))
            .await?;
        Ok(())
    }

    /// Grant +o to a user in a channel via MODE.
    ///
    /// Note: Shared across multiple integration tests; clippy may flag it
    /// as unused in a single test binary.
    #[allow(dead_code)]
    pub async fn mode_channel_op(&mut self, channel: &str, nick: &str) -> anyhow::Result<()> {
        // Use raw MODE to avoid guessing slirc-proto variant shapes.
        self.send_raw(&format!("MODE {} +o {}", channel, nick))
            .await
    }
}

async fn connect_tls_stream(
    stream: TcpStream,
    tls: &TlsClientConfig,
) -> anyhow::Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let ca_data = tokio::fs::read(&tls.ca_path).await?;
    let mut ca_reader = StdBufReader::new(Cursor::new(ca_data));
    let ca_certs: Vec<CertificateDer> = certs(&mut ca_reader).collect::<Result<Vec<_>, _>>()?;

    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store.add(cert)?;
    }

    let config = if let (Some(cert_path), Some(key_path)) =
        (tls.client_cert_path.as_ref(), tls.client_key_path.as_ref())
    {
        let cert_data = tokio::fs::read(cert_path).await?;
        let mut cert_reader = StdBufReader::new(Cursor::new(cert_data));
        let certs: Vec<CertificateDer> = certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

        let key_data = tokio::fs::read(key_path).await?;
        let mut key_reader = StdBufReader::new(Cursor::new(key_data));
        let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(PrivateKeyDer::from)
            .collect();

        if keys.is_empty() {
            anyhow::bail!("No private keys found in {}", key_path.display());
        }

        let key = keys.remove(0);
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, key)?
    } else {
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };

    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(tls.server_name.clone())
        .map_err(|e| anyhow::anyhow!("Invalid TLS server name: {e}"))?;
    let tls_stream = connector.connect(server_name, stream).await?;
    Ok(tls_stream)
}
