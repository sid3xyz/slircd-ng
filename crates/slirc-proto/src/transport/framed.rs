//! Framed IRC transport over TCP, TLS, and WebSocket.

use anyhow::Result;
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_util::codec::Framed;
use tracing::warn;

use crate::error::ProtocolError;
use crate::irc::IrcCodec;
use crate::Message;

use super::error::TransportReadError;
use super::parts::{TransportParts, TransportStream};
use super::MAX_IRC_LINE_LEN;

#[cfg(feature = "tokio")]
use tokio_tungstenite::{tungstenite::Message as WsMessage, WebSocketStream};

/// IRC transport over various stream types.
///
/// Supports TCP, TLS, and WebSocket connections. Use during connection
/// handshake and capability negotiation, then convert to [`super::ZeroCopyTransportEnum`]
/// for high-performance message processing.
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum Transport {
    /// Plain TCP transport.
    Tcp {
        /// The framed codec for TCP.
        framed: Framed<tokio::net::TcpStream, IrcCodec>,
    },
    /// Server-side TLS-encrypted transport.
    ///
    /// Use this for IRC servers accepting TLS connections.
    Tls {
        /// The framed codec for server-side TLS.
        framed: Framed<ServerTlsStream<TcpStream>, IrcCodec>,
    },
    /// Client-side TLS-encrypted transport.
    ///
    /// Use this for IRC clients connecting to TLS-enabled servers.
    ClientTls {
        /// The framed codec for client-side TLS.
        framed: Framed<ClientTlsStream<TcpStream>, IrcCodec>,
    },
    /// WebSocket transport (plain).
    #[cfg(feature = "tokio")]
    WebSocket {
        /// The WebSocket stream.
        stream: WebSocketStream<TcpStream>,
    },
    /// WebSocket transport over TLS.
    #[cfg(feature = "tokio")]
    WebSocketTls {
        /// The TLS WebSocket stream.
        stream: WebSocketStream<ServerTlsStream<TcpStream>>,
    },
}

/// Error returned when converting a WebSocket transport to zero-copy.
///
/// WebSocket transports cannot be converted to zero-copy because the
/// WebSocket framing protocol requires different handling.
pub struct WebSocketNotSupportedError;

impl std::fmt::Debug for WebSocketNotSupportedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "WebSocket transport cannot be converted to zero-copy transport"
        )
    }
}

impl std::fmt::Display for WebSocketNotSupportedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WebSocket transport cannot be converted to zero-copy transport"
        )
    }
}

impl std::error::Error for WebSocketNotSupportedError {}

impl Transport {
    /// Create a new TCP transport from a connected stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the UTF-8 codec cannot be created (should not happen
    /// in practice, but avoids panicking in library code).
    pub fn tcp(stream: TcpStream) -> Result<Self, ProtocolError> {
        if let Err(e) = Self::enable_keepalive(&stream) {
            warn!("failed to enable TCP keepalive: {}", e);
        }

        let codec = IrcCodec::new("utf-8")?;
        Ok(Self::Tcp {
            framed: Framed::new(stream, codec),
        })
    }

    fn enable_keepalive(stream: &TcpStream) -> Result<()> {
        use socket2::{SockRef, TcpKeepalive};
        use std::time::Duration;

        let sock = SockRef::from(stream);
        let keepalive = TcpKeepalive::new()
            .with_time(Duration::from_secs(120))
            .with_interval(Duration::from_secs(30));

        sock.set_tcp_keepalive(&keepalive)?;
        Ok(())
    }

    /// Create a new server-side TLS transport from an established TLS stream.
    ///
    /// This is typically used by IRC servers accepting incoming TLS connections.
    ///
    /// # Errors
    ///
    /// Returns an error if the UTF-8 codec cannot be created (should not happen
    /// in practice, but avoids panicking in library code).
    pub fn tls(stream: ServerTlsStream<TcpStream>) -> Result<Self, ProtocolError> {
        let codec = IrcCodec::new("utf-8")?;
        Ok(Self::Tls {
            framed: Framed::new(stream, codec),
        })
    }

    /// Create a new client-side TLS transport from an established TLS stream.
    ///
    /// This is typically used by IRC clients connecting to TLS-enabled servers.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use slirc_proto::transport::Transport;
    /// use tokio_rustls::TlsConnector;
    /// use tokio::net::TcpStream;
    ///
    /// let tcp_stream = TcpStream::connect("irc.libera.chat:6697").await?;
    /// let tls_stream = connector.connect(server_name, tcp_stream).await?;
    /// let transport = Transport::client_tls(tls_stream)?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the UTF-8 codec cannot be created (should not happen
    /// in practice, but avoids panicking in library code).
    pub fn client_tls(stream: ClientTlsStream<TcpStream>) -> Result<Self, ProtocolError> {
        let codec = IrcCodec::new("utf-8")?;
        Ok(Self::ClientTls {
            framed: Framed::new(stream, codec),
        })
    }

    /// Create a new WebSocket transport.
    #[cfg(feature = "tokio")]
    pub fn websocket(stream: WebSocketStream<TcpStream>) -> Self {
        Self::WebSocket { stream }
    }

    /// Create a new WebSocket transport over TLS.
    #[cfg(feature = "tokio")]
    pub fn websocket_tls(stream: WebSocketStream<ServerTlsStream<TcpStream>>) -> Self {
        Self::WebSocketTls { stream }
    }

    /// Consume the `Transport`, returning the underlying raw stream and any
    /// buffered bytes that were read but not yet parsed. This is intended for
    /// callers that want to take over I/O (for example to spawn a writer task)
    /// while preserving any buffered data for a zero-copy reader.
    pub fn into_parts(self) -> Result<TransportParts, WebSocketNotSupportedError> {
        match self {
            Transport::Tcp { framed } => {
                let parts = framed.into_parts();
                Ok(TransportParts {
                    stream: TransportStream::Tcp(parts.io),
                    read_buf: parts.read_buf,
                    write_buf: parts.write_buf,
                    codec: parts.codec,
                })
            }
            Transport::Tls { framed } => {
                let parts = framed.into_parts();
                Ok(TransportParts {
                    stream: TransportStream::Tls(Box::new(parts.io)),
                    read_buf: parts.read_buf,
                    write_buf: parts.write_buf,
                    codec: parts.codec,
                })
            }
            Transport::ClientTls { framed } => {
                let parts = framed.into_parts();
                Ok(TransportParts {
                    stream: TransportStream::ClientTls(Box::new(parts.io)),
                    read_buf: parts.read_buf,
                    write_buf: parts.write_buf,
                    codec: parts.codec,
                })
            }
            #[cfg(feature = "tokio")]
            Transport::WebSocket { stream } => Ok(TransportParts {
                stream: TransportStream::WebSocket(Box::new(stream)),
                read_buf: BytesMut::new(),
                write_buf: BytesMut::new(),
                codec: IrcCodec::new("utf-8").map_err(|_| WebSocketNotSupportedError)?,
            }),
            #[cfg(feature = "tokio")]
            Transport::WebSocketTls { stream } => Ok(TransportParts {
                stream: TransportStream::WebSocketTls(Box::new(stream)),
                read_buf: BytesMut::new(),
                write_buf: BytesMut::new(),
                codec: IrcCodec::new("utf-8").map_err(|_| WebSocketNotSupportedError)?,
            }),
        }
    }

    /// Check if this transport uses TLS encryption (either server or client).
    pub fn is_tls(&self) -> bool {
        matches!(self, Self::Tls { .. } | Self::ClientTls { .. })
    }

    /// Check if this transport uses client-side TLS.
    pub fn is_client_tls(&self) -> bool {
        matches!(self, Self::ClientTls { .. })
    }

    /// Check if this transport uses server-side TLS.
    pub fn is_server_tls(&self) -> bool {
        matches!(self, Self::Tls { .. })
    }

    /// Check if this transport uses WebSocket framing.
    pub fn is_websocket(&self) -> bool {
        #[cfg(feature = "tokio")]
        {
            matches!(self, Self::WebSocket { .. } | Self::WebSocketTls { .. })
        }
        #[cfg(not(feature = "tokio"))]
        {
            false
        }
    }

    /// Read the next IRC message from the transport.
    ///
    /// Returns `Ok(None)` when the connection is closed.
    pub async fn read_message(&mut self) -> Result<Option<Message>, TransportReadError> {
        macro_rules! read_framed {
            ($framed:expr) => {
                match $framed.next().await {
                    Some(Ok(msg)) => Ok(Some(msg)),
                    Some(Err(e)) => Err(TransportReadError::from(e)),
                    None => Ok(None),
                }
            };
        }

        macro_rules! read_websocket {
            ($stream:expr) => {{
                let text = read_websocket_message($stream).await?;
                match text {
                    Some(s) => s
                        .parse::<Message>()
                        .map(Some)
                        .map_err(TransportReadError::from),
                    None => Ok(None),
                }
            }};
        }

        match self {
            Transport::Tcp { framed } => read_framed!(framed),
            Transport::Tls { framed } => read_framed!(framed),
            Transport::ClientTls { framed } => read_framed!(framed),
            #[cfg(feature = "tokio")]
            Transport::WebSocket { stream } => read_websocket!(stream),
            #[cfg(feature = "tokio")]
            Transport::WebSocketTls { stream } => read_websocket!(stream),
        }
    }

    /// Write an IRC message to the transport.
    pub async fn write_message(&mut self, message: &Message) -> Result<()> {
        macro_rules! write_framed {
            ($framed:expr, $msg:expr) => {
                $framed
                    .send($msg.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            };
        }

        match self {
            Transport::Tcp { framed } => write_framed!(framed, message),
            Transport::Tls { framed } => write_framed!(framed, message),
            Transport::ClientTls { framed } => write_framed!(framed, message),
            #[cfg(feature = "tokio")]
            Transport::WebSocket { stream } => write_websocket_message(stream, message).await,
            #[cfg(feature = "tokio")]
            Transport::WebSocketTls { stream } => write_websocket_message(stream, message).await,
        }
    }
}

#[cfg(feature = "tokio")]
async fn read_websocket_message<S>(
    stream: &mut WebSocketStream<S>,
) -> Result<Option<String>, TransportReadError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        match stream.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                if text.len() > MAX_IRC_LINE_LEN {
                    return Err(TransportReadError::Protocol(
                        ProtocolError::MessageTooLong {
                            actual: text.len(),
                            limit: MAX_IRC_LINE_LEN,
                        },
                    ));
                }

                let trimmed = text.trim_end_matches(&['\r', '\n'][..]);

                for ch in trimmed.chars() {
                    if crate::format::is_illegal_control_char(ch) {
                        return Err(TransportReadError::Protocol(
                            ProtocolError::IllegalControlChar(ch),
                        ));
                    }
                }

                return Ok(Some(trimmed.to_string()));
            }
            Some(Ok(WsMessage::Close(_))) | None => {
                return Ok(None);
            }
            Some(Ok(WsMessage::Ping(_))) | Some(Ok(WsMessage::Pong(_))) => {
                continue;
            }
            Some(Ok(WsMessage::Binary(_))) => {
                warn!("Ignoring binary WebSocket frame (IRC is text-only)");
                continue;
            }
            Some(Ok(WsMessage::Frame(_))) => {
                continue;
            }
            Some(Err(e)) => {
                return Err(TransportReadError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("WebSocket error: {}", e),
                )));
            }
        }
    }
}

#[cfg(feature = "tokio")]
async fn write_websocket_message<S>(
    stream: &mut WebSocketStream<S>,
    message: &Message,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use std::fmt::Write;
    let mut msg = String::with_capacity(512);
    write!(&mut msg, "{}", message).expect("fmt::Write to String cannot fail");

    // Trim trailing CRLF
    let len = msg.trim_end_matches(&['\r', '\n'][..]).len();
    msg.truncate(len);

    stream
        .send(WsMessage::Text(msg))
        .await
        .map_err(|e| anyhow::anyhow!("WebSocket send error: {}", e))?;
    Ok(())
}
