//! Server-to-Server stream abstraction.
//!
//! Provides a unified stream type for both plaintext and TLS S2S connections.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;

/// A unified stream type for server-to-server connections.
///
/// This enum wraps both plaintext TCP streams and TLS-encrypted streams,
/// allowing the protocol layer to be agnostic to the transport security.
pub enum S2SStream {
    /// Plaintext TCP connection.
    Plain(TcpStream),
    /// TLS-encrypted client connection (outbound).
    TlsClient(ClientTlsStream<TcpStream>),
    /// TLS-encrypted server connection (inbound).
    #[allow(dead_code)] // Will be used when inbound TLS listener is implemented
    TlsServer(ServerTlsStream<TcpStream>),
}

impl S2SStream {
    /// Returns true if this is a TLS-encrypted connection.
    #[allow(dead_code)]
    pub fn is_tls(&self) -> bool {
        !matches!(self, Self::Plain(_))
    }
}

impl AsyncRead for S2SStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            S2SStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            S2SStream::TlsClient(stream) => Pin::new(stream).poll_read(cx, buf),
            S2SStream::TlsServer(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for S2SStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            S2SStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            S2SStream::TlsClient(stream) => Pin::new(stream).poll_write(cx, buf),
            S2SStream::TlsServer(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            S2SStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            S2SStream::TlsClient(stream) => Pin::new(stream).poll_flush(cx),
            S2SStream::TlsServer(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            S2SStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            S2SStream::TlsClient(stream) => Pin::new(stream).poll_shutdown(cx),
            S2SStream::TlsServer(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

// S2SStream is Unpin because all variants contain Unpin types
impl Unpin for S2SStream {}
