//! Transport parts for splitting read/write halves.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;

use crate::irc::IrcCodec;

#[cfg(feature = "tokio")]
use tokio_tungstenite::WebSocketStream;

/// A unified raw transport stream type for hand-off to users.
#[non_exhaustive]
pub enum TransportStream {
    /// Plain TCP stream.
    Tcp(TcpStream),
    /// Server-side TLS stream (boxed for size).
    Tls(Box<ServerTlsStream<TcpStream>>),
    /// Client-side TLS stream (boxed for size).
    ClientTls(Box<ClientTlsStream<TcpStream>>),
    /// WebSocket stream (plain).
    #[cfg(feature = "tokio")]
    WebSocket(Box<WebSocketStream<TcpStream>>),
    /// WebSocket stream over TLS.
    #[cfg(feature = "tokio")]
    WebSocketTls(Box<WebSocketStream<ServerTlsStream<TcpStream>>>),
}

/// The parts extracted from a `Transport`, including any buffered data
/// that has already been read but not yet parsed.
pub struct TransportParts {
    /// The underlying raw stream.
    pub stream: TransportStream,
    /// Bytes read but not yet parsed.
    pub read_buf: BytesMut,
    /// Bytes waiting to be written.
    pub write_buf: BytesMut,
    /// The IRC codec for message framing.
    pub codec: IrcCodec,
}

/// Owned read half for a transport after splitting.
pub enum TransportReadHalf {
    /// TCP read half.
    Tcp(tokio::net::tcp::OwnedReadHalf),
    /// Server-side TLS read half.
    Tls(tokio::io::ReadHalf<ServerTlsStream<TcpStream>>),
    /// Client-side TLS read half.
    ClientTls(tokio::io::ReadHalf<ClientTlsStream<TcpStream>>),
}

/// Owned write half for a transport after splitting.
pub enum TransportWriteHalf {
    /// TCP write half.
    Tcp(tokio::net::tcp::OwnedWriteHalf),
    /// Server-side TLS write half.
    Tls(tokio::io::WriteHalf<ServerTlsStream<TcpStream>>),
    /// Client-side TLS write half.
    ClientTls(tokio::io::WriteHalf<ClientTlsStream<TcpStream>>),
}

impl AsyncRead for TransportReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(inner) => Pin::new(inner).poll_read(cx, buf),
            Self::Tls(inner) => Pin::new(inner).poll_read(cx, buf),
            Self::ClientTls(inner) => Pin::new(inner).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TransportWriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Tcp(inner) => Pin::new(inner).poll_write(cx, buf),
            Self::Tls(inner) => Pin::new(inner).poll_write(cx, buf),
            Self::ClientTls(inner) => Pin::new(inner).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(inner) => Pin::new(inner).poll_flush(cx),
            Self::Tls(inner) => Pin::new(inner).poll_flush(cx),
            Self::ClientTls(inner) => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(inner) => Pin::new(inner).poll_shutdown(cx),
            Self::Tls(inner) => Pin::new(inner).poll_shutdown(cx),
            Self::ClientTls(inner) => Pin::new(inner).poll_shutdown(cx),
        }
    }
}

/// A convenience container for a split transport read side with any pre-seeded
/// buffer loaded from the original framed transport.
pub struct TransportRead {
    /// The read half of the transport.
    pub half: TransportReadHalf,
    /// Bytes read but not yet parsed.
    pub read_buf: BytesMut,
}

/// A convenience container for a split transport write side including any
/// write buffer and codec to reconstruct a framed writer.
pub struct TransportWrite {
    /// The write half of the transport.
    pub half: TransportWriteHalf,
    /// Bytes waiting to be written.
    pub write_buf: BytesMut,
    /// The IRC codec for message framing.
    pub codec: IrcCodec,
}

impl TransportParts {
    /// Split the `TransportParts` into read & write halves suitable for
    /// spawning separate tasks. The read half contains any leftover bytes
    /// that were read but not yet parsed; the write half contains the
    /// codec and write buffer allowing the caller to create a framed sink.
    pub fn split(self) -> (TransportRead, TransportWrite) {
        match self.stream {
            TransportStream::Tcp(stream) => {
                let (r, w) = stream.into_split();
                (
                    TransportRead {
                        half: TransportReadHalf::Tcp(r),
                        read_buf: self.read_buf,
                    },
                    TransportWrite {
                        half: TransportWriteHalf::Tcp(w),
                        write_buf: self.write_buf,
                        codec: self.codec,
                    },
                )
            }
            TransportStream::Tls(stream) => {
                // Unbox and split the server-side TLS stream
                let (r, w) = tokio::io::split(*stream);
                (
                    TransportRead {
                        half: TransportReadHalf::Tls(r),
                        read_buf: self.read_buf,
                    },
                    TransportWrite {
                        half: TransportWriteHalf::Tls(w),
                        write_buf: self.write_buf,
                        codec: self.codec,
                    },
                )
            }
            TransportStream::ClientTls(stream) => {
                // Unbox and split the client-side TLS stream
                let (r, w) = tokio::io::split(*stream);
                (
                    TransportRead {
                        half: TransportReadHalf::ClientTls(r),
                        read_buf: self.read_buf,
                    },
                    TransportWrite {
                        half: TransportWriteHalf::ClientTls(w),
                        write_buf: self.write_buf,
                        codec: self.codec,
                    },
                )
            }
            #[cfg(feature = "tokio")]
            TransportStream::WebSocket(_ws) => {
                // WebSocket streams don't have a sensible split that maintains
                // the line-based message semantics; return sink/stream halves.
                // We intentionally panic here to make unsupported usage explicit.
                panic!("WebSocket split not supported via TransportParts::split");
            }
            #[cfg(feature = "tokio")]
            TransportStream::WebSocketTls(_ws) => {
                panic!("WebSocketTls split not supported via TransportParts::split");
            }
        }
    }
}
