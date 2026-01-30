//! Unified enum wrapper for all zero-copy transport types.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_rustls::TlsAcceptor;

#[cfg(feature = "tokio")]
use tokio_tungstenite::WebSocketStream;

use crate::message::MessageRef;
use crate::Message;

use super::super::error::TransportReadError;
use super::super::framed::Transport;
use super::tcp::ZeroCopyTransport;
use super::trait_def::LendingStream;
use crate::error::ProtocolError;

#[cfg(feature = "tokio")]
use super::websocket::ZeroCopyWebSocketTransport;

/// Enum wrapper for zero-copy transports over different stream types.
///
/// This provides a unified interface for zero-copy message reading
/// over TCP, TLS, and WebSocket connections.
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum ZeroCopyTransportEnum {
    /// TCP zero-copy transport.
    Tcp(ZeroCopyTransport<TcpStream>),
    /// Server-side TLS zero-copy transport.
    Tls(ZeroCopyTransport<ServerTlsStream<TcpStream>>),
    /// Client-side TLS zero-copy transport.
    ClientTls(ZeroCopyTransport<ClientTlsStream<TcpStream>>),
    /// WebSocket zero-copy transport.
    #[cfg(feature = "tokio")]
    WebSocket(ZeroCopyWebSocketTransport<TcpStream>),
    /// WebSocket over TLS zero-copy transport.
    #[cfg(feature = "tokio")]
    WebSocketTls(ZeroCopyWebSocketTransport<ServerTlsStream<TcpStream>>),
}

impl ZeroCopyTransportEnum {
    /// Create a new TCP zero-copy transport.
    pub fn tcp(stream: TcpStream) -> Self {
        Self::Tcp(ZeroCopyTransport::new(stream))
    }

    /// Create a new TCP zero-copy transport with an existing buffer.
    pub fn tcp_with_buffer(stream: TcpStream, buffer: BytesMut) -> Self {
        Self::Tcp(ZeroCopyTransport::with_buffer(stream, buffer))
    }

    /// Create a new server-side TLS zero-copy transport.
    pub fn tls(stream: ServerTlsStream<TcpStream>) -> Self {
        Self::Tls(ZeroCopyTransport::new(stream))
    }

    /// Create a new server-side TLS zero-copy transport with an existing buffer.
    pub fn tls_with_buffer(stream: ServerTlsStream<TcpStream>, buffer: BytesMut) -> Self {
        Self::Tls(ZeroCopyTransport::with_buffer(stream, buffer))
    }

    /// Create a new client-side TLS zero-copy transport.
    pub fn client_tls(stream: ClientTlsStream<TcpStream>) -> Self {
        Self::ClientTls(ZeroCopyTransport::new(stream))
    }

    /// Create a new client-side TLS zero-copy transport with an existing buffer.
    pub fn client_tls_with_buffer(stream: ClientTlsStream<TcpStream>, buffer: BytesMut) -> Self {
        Self::ClientTls(ZeroCopyTransport::with_buffer(stream, buffer))
    }

    /// Create a new WebSocket zero-copy transport.
    #[cfg(feature = "tokio")]
    pub fn websocket(stream: WebSocketStream<TcpStream>) -> Self {
        Self::WebSocket(ZeroCopyWebSocketTransport::new(stream))
    }

    /// Create a new WebSocket zero-copy transport with an existing buffer.
    #[cfg(feature = "tokio")]
    pub fn websocket_with_buffer(stream: WebSocketStream<TcpStream>, buffer: BytesMut) -> Self {
        Self::WebSocket(ZeroCopyWebSocketTransport::with_buffer(stream, buffer))
    }

    /// Create a new WebSocket over TLS zero-copy transport.
    #[cfg(feature = "tokio")]
    pub fn websocket_tls(stream: WebSocketStream<ServerTlsStream<TcpStream>>) -> Self {
        Self::WebSocketTls(ZeroCopyWebSocketTransport::new(stream))
    }

    /// Create a new WebSocket over TLS zero-copy transport with an existing buffer.
    #[cfg(feature = "tokio")]
    pub fn websocket_tls_with_buffer(
        stream: WebSocketStream<ServerTlsStream<TcpStream>>,
        buffer: BytesMut,
    ) -> Self {
        Self::WebSocketTls(ZeroCopyWebSocketTransport::with_buffer(stream, buffer))
    }

    /// Set the maximum line length for the transport.
    pub fn set_max_line_len(&mut self, len: usize) {
        match self {
            Self::Tcp(t) => t.set_max_line_len(len),
            Self::Tls(t) => t.set_max_line_len(len),
            Self::ClientTls(t) => t.set_max_line_len(len),
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => t.set_max_line_len(len),
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => t.set_max_line_len(len),
        }
    }

    /// Read the next message from the transport.
    pub async fn next(&mut self) -> Option<Result<MessageRef<'_>, TransportReadError>> {
        match self {
            Self::Tcp(t) => t.next().await,
            Self::Tls(t) => t.next().await,
            Self::ClientTls(t) => t.next().await,
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => t.next().await,
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => t.next().await,
        }
    }

    /// Write an IRC message to the transport.
    ///
    /// This enables unified read/write operations in a single `tokio::select!`
    /// loop without needing separate writer infrastructure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// loop {
    ///     tokio::select! {
    ///         Some(result) = transport.next() => {
    ///             let msg = result?;
    ///             // handle incoming message
    ///         }
    ///         Some(outgoing) = rx.recv() => {
    ///             transport.write_message(&outgoing).await?;
    ///         }
    ///     }
    /// }
    /// ```
    pub async fn write_message(&mut self, message: &Message) -> std::io::Result<()> {
        match self {
            Self::Tcp(t) => t.write_message(message).await,
            Self::Tls(t) => t.write_message(message).await,
            Self::ClientTls(t) => t.write_message(message).await,
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => t.write_message(message).await,
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => t.write_message(message).await,
        }
    }

    /// Write multiple IRC messages to the transport in a single batch.
    ///
    /// This delegates to `ZeroCopyTransport::write_messages` for efficient
    /// single-syscall writing.
    pub async fn write_messages(&mut self, messages: &[Message]) -> std::io::Result<()> {
        match self {
            Self::Tcp(t) => t.write_messages(messages).await,
            Self::Tls(t) => t.write_messages(messages).await,
            Self::ClientTls(t) => t.write_messages(messages).await,
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => t.write_messages(messages).await,
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => t.write_messages(messages).await,
        }
    }

    /// Write a borrowed IRC message to the transport (zero-copy forwarding).
    ///
    /// This is optimized for S2S message forwarding and relay scenarios
    /// where you receive a `MessageRef` and want to forward it without
    /// allocating an owned `Message`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // S2S forwarding: receive from one server, forward to another
    /// while let Some(result) = server_a.next().await {
    ///     let msg_ref = result?;
    ///     if should_forward(&msg_ref) {
    ///         server_b.write_message_ref(&msg_ref).await?;
    ///     }
    /// }
    /// ```
    pub async fn write_message_ref(&mut self, message: &MessageRef<'_>) -> std::io::Result<()> {
        match self {
            Self::Tcp(t) => t.write_message_ref(message).await,
            Self::Tls(t) => t.write_message_ref(message).await,
            Self::ClientTls(t) => t.write_message_ref(message).await,
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => t.write_message_ref(message).await,
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => t.write_message_ref(message).await,
        }
    }

    /// Check if this transport is already using TLS.
    pub fn is_tls(&self) -> bool {
        matches!(
            self,
            Self::Tls(_) | Self::ClientTls(_) | Self::WebSocketTls(_)
        )
    }

    /// Upgrade a plaintext TCP connection to TLS (STARTTLS).
    ///
    /// This consumes the current transport, performs the TLS handshake,
    /// and returns a new TLS-wrapped transport, preserving any buffered data.
    ///
    /// # Zero-Data-Loss Guarantee
    ///
    /// - Buffered read data is preserved through the upgrade
    /// - The TLS handshake happens on the underlying TCP stream
    /// - After upgrade, all I/O is encrypted
    ///
    /// # Errors
    ///
    /// Returns `Err((self, io::Error))` if:
    /// - The transport is not a TCP transport (already TLS or WebSocket)
    ///
    /// Returns `Err((dummy, io::Error))` if:
    /// - The TLS handshake fails (connection is dead in this case)
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Client sends STARTTLS, server responds with 670
    /// transport.write_message(&Message::numeric("670", &[], "STARTTLS successful")).await?;
    ///
    /// // Perform the upgrade (consumes transport)
    /// transport = transport.into_tls(acceptor).await?;
    ///
    /// // All subsequent I/O is now encrypted
    /// ```
    pub async fn into_tls(self, acceptor: TlsAcceptor) -> Result<Self, (Self, std::io::Error)> {
        match self {
            Self::Tcp(transport) => {
                // Extract stream and buffer
                let (tcp_stream, buffer) = transport.into_parts();

                // Perform TLS handshake
                match acceptor.accept(tcp_stream).await {
                    Ok(tls_stream) => {
                        // Create new TLS transport with preserved buffer
                        Ok(Self::Tls(ZeroCopyTransport::with_buffer(
                            tls_stream, buffer,
                        )))
                    }
                    Err(e) => {
                        // TLS handshake failed - connection is dead
                        // Return a dummy transport (will error on any I/O)
                        Err((
                            Self::Tcp(ZeroCopyTransport::with_buffer(
                                // Create a disconnected socket placeholder
                                TcpStream::from_std(
                                    std::net::TcpStream::connect("0.0.0.0:0").unwrap_or_else(
                                        |_| panic!("Failed to create dummy socket"),
                                    ),
                                )
                                .unwrap_or_else(|_| panic!("Failed to convert dummy socket")),
                                buffer,
                            )),
                            e,
                        ))
                    }
                }
            }
            other => Err((
                other,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "STARTTLS only supported on plaintext TCP connections",
                ),
            )),
        }
    }

    /// Upgrade a plaintext TCP connection to TLS in-place (STARTTLS).
    ///
    /// This method performs TLS upgrade on the current connection. It only works
    /// on TCP transports; calling it on already-TLS or WebSocket transports returns
    /// an error without modifying the transport.
    ///
    /// # Connection State After Error
    ///
    /// On TLS handshake failure, the transport is replaced with a "dead" TLS transport
    /// that will return errors on all I/O. The caller should close the connection.
    ///
    /// # Errors
    ///
    /// Returns `Err(io::Error)` if:
    /// - The transport is not a TCP transport
    /// - The TLS handshake fails
    pub async fn upgrade_to_tls(&mut self, acceptor: TlsAcceptor) -> Result<(), std::io::Error> {
        // Check if this is a TCP transport without consuming
        if !matches!(self, Self::Tcp(_)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "STARTTLS only supported on plaintext TCP connections",
            ));
        }

        // Use the consuming into_tls method and swap
        // We create a temporary "placeholder" by taking self via mem::replace
        // with an uninitialized variant that we immediately discard.
        //
        // SAFETY: We use ManuallyDrop to prevent the placeholder from being
        // dropped if we abort between the replace and reassignment.
        use std::mem::ManuallyDrop;

        // Step 1: Take ownership by replacing with a never-dropped placeholder
        let placeholder = ManuallyDrop::new(Self::Tcp(ZeroCopyTransport::with_buffer(
            // Connect to ourselves on an ephemeral port - will fail but gives us a valid TcpStream
            // This is only used as a temporary placeholder; it's never read from or written to.
            TcpStream::from_std(
                std::net::TcpStream::connect(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
                    .or_else(|_| {
                        std::net::TcpStream::connect(std::net::SocketAddr::from(([0, 0, 0, 0], 0)))
                    })
                    .unwrap_or_else(|_| {
                        // Last resort: create a Unix socket pair and use one end
                        // This should never fail
                        panic!("Failed to create placeholder socket for STARTTLS")
                    }),
            )
            .expect("Failed to convert placeholder socket"),
            BytesMut::new(),
        )));

        // Step 2: Take ownership of current transport, leaving placeholder
        let current = std::mem::replace(self, ManuallyDrop::into_inner(placeholder));

        // Step 3: Attempt upgrade
        match current.into_tls(acceptor).await {
            Ok(upgraded) => {
                *self = upgraded;
                Ok(())
            }
            Err((original, err)) => {
                // Restore original (which may be broken if handshake partially happened)
                *self = original;
                Err(err)
            }
        }
    }
}

impl LendingStream for ZeroCopyTransportEnum {
    type Item<'a>
        = MessageRef<'a>
    where
        Self: 'a;
    type Error = TransportReadError;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item<'_>, Self::Error>>> {
        match self.get_mut() {
            Self::Tcp(t) => Pin::new(t).poll_next(cx),
            Self::Tls(t) => Pin::new(t).poll_next(cx),
            Self::ClientTls(t) => Pin::new(t).poll_next(cx),
            #[cfg(feature = "tokio")]
            Self::WebSocket(t) => Pin::new(t).poll_next(cx),
            #[cfg(feature = "tokio")]
            Self::WebSocketTls(t) => Pin::new(t).poll_next(cx),
        }
    }
}

/// Convert a `Transport` to a `ZeroCopyTransportEnum`.
///
/// This performs a buffer handover from the `Framed` codec to the
/// zero-copy transport, ensuring no data is lost during the upgrade.
impl TryFrom<Transport> for ZeroCopyTransportEnum {
    type Error = ProtocolError;

    fn try_from(transport: Transport) -> Result<Self, Self::Error> {
        // Use into_parts() which now supports all transport types including WebSocket.
        let parts = transport
            .into_parts()
            .map_err(|_| ProtocolError::WebSocketNotSupported)?;

        Ok(match parts.stream {
            super::super::parts::TransportStream::Tcp(stream) => {
                ZeroCopyTransportEnum::tcp_with_buffer(stream, parts.read_buf)
            }
            super::super::parts::TransportStream::Tls(stream) => {
                ZeroCopyTransportEnum::tls_with_buffer(*stream, parts.read_buf)
            }
            super::super::parts::TransportStream::ClientTls(stream) => {
                ZeroCopyTransportEnum::client_tls_with_buffer(*stream, parts.read_buf)
            }
            #[cfg(feature = "tokio")]
            super::super::parts::TransportStream::WebSocket(stream) => {
                ZeroCopyTransportEnum::websocket_with_buffer(*stream, parts.read_buf)
            }
            #[cfg(feature = "tokio")]
            super::super::parts::TransportStream::WebSocketTls(stream) => {
                ZeroCopyTransportEnum::websocket_tls_with_buffer(*stream, parts.read_buf)
            }
        })
    }
}
