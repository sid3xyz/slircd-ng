//! IRC transport layer for async I/O.
//!
//! This module provides transport types for reading and writing IRC messages
//! over TCP, TLS (server and client), and WebSocket connections.
//!
//! # Features
//!
//! - [`Transport`]: High-level transport using `Framed` codec for owned [`Message`] types
//!   - [`Transport::tcp`]: Plain TCP connections
//!   - [`Transport::tls`]: Server-side TLS (for IRC servers)
//!   - [`Transport::client_tls`]: Client-side TLS (for IRC clients connecting to port 6697)
//!   - [`Transport::websocket`] / [`Transport::websocket_tls`]: WebSocket connections
//! - [`ZeroCopyTransport`]: Zero-allocation transport yielding borrowed [`MessageRef`] types
//! - [`LendingStream`]: Trait for streams that yield borrowed data
//!
//! # Usage
//!
//! Use [`Transport`] during connection handshake and capability negotiation,
//! then upgrade to [`ZeroCopyTransport`] for the hot loop:
//!
//! ```ignore
//! use slirc_proto::transport::{Transport, ZeroCopyTransportEnum};
//!
//! // Use Transport during handshake
//! let transport = Transport::tcp(stream);
//! // ... perform CAP negotiation ...
//!
//! // Upgrade to zero-copy for the hot loop
//! let mut zero_copy: ZeroCopyTransportEnum = transport.try_into()?;
//! while let Some(result) = zero_copy.next().await {
//!     let msg_ref = result?;
//!     // Process MessageRef without allocations
//! }
//!
//! // If you want to split the stream into separate read and write halves while
//! // preserving any bytes that were already read by the framed codec, use
//! // `Transport::into_parts()`:
//! //
//! // ```ignore
//! // // After handshake
//! // let parts = transport.into_parts()?;
//! // // Split into read/write halves
//! // let (read, write) = parts.split();
//! // // Seed the zero-copy reader with leftover bytes
//! // let mut zero_copy = ZeroCopyTransport::with_buffer(read.half, read.read_buf);
//! // // Create a framed writer using the write half and codec
//! // let mut writer = tokio_util::codec::FramedWrite::new(write.half, write.codec);
//! // ```
//! ```
//!
//! [`Message`]: crate::Message
//! [`MessageRef`]: crate::MessageRef

mod error;
mod framed;
mod parts;
mod zero_copy;

// Re-export all public types
pub use error::TransportReadError;
pub use framed::{Transport, WebSocketNotSupportedError};
pub use parts::{
    TransportParts, TransportRead, TransportReadHalf, TransportStream, TransportWrite,
    TransportWriteHalf,
};
#[cfg(feature = "tokio")]
pub use zero_copy::ZeroCopyWebSocketTransport;
pub use zero_copy::{LendingStream, ZeroCopyTransport, ZeroCopyTransportEnum};

/// Maximum IRC line length (8191 bytes as per modern IRC conventions).
pub const MAX_IRC_LINE_LEN: usize = 8191;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::io::Cursor;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncRead;

    /// A mock async reader that returns data from a byte slice.
    struct MockReader {
        data: Cursor<Vec<u8>>,
    }

    impl MockReader {
        fn new(data: &[u8]) -> Self {
            Self {
                data: Cursor::new(data.to_vec()),
            }
        }
    }

    impl AsyncRead for MockReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let pos = self.data.position() as usize;
            let data = self.data.get_ref();
            if pos >= data.len() {
                return Poll::Ready(Ok(()));
            }
            let to_read = (data.len() - pos).min(buf.remaining());
            buf.put_slice(&data[pos..pos + to_read]);
            self.data.set_position((pos + to_read) as u64);
            Poll::Ready(Ok(()))
        }
    }

    impl Unpin for MockReader {}

    #[tokio::test]
    async fn test_zero_copy_simple() {
        let data = b"PING :server\r\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        let result = transport.next().await;
        assert!(result.is_some());
        let msg = result.unwrap().unwrap();
        assert_eq!(msg.command_name(), "PING");
        assert_eq!(msg.args(), &["server"]);
    }

    #[tokio::test]
    async fn test_zero_copy_multiple_messages() {
        let data = b"PING :server1\r\nPING :server2\r\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        // Process first message and drop it before calling next() again
        {
            let msg1 = transport.next().await.unwrap().unwrap();
            assert_eq!(msg1.args(), &["server1"]);
        }

        {
            let msg2 = transport.next().await.unwrap().unwrap();
            assert_eq!(msg2.args(), &["server2"]);
        }

        let msg3 = transport.next().await;
        assert!(msg3.is_none());
    }

    #[tokio::test]
    async fn test_zero_copy_with_tags() {
        let data = b"@time=2023-01-01;msgid=abc :nick!user@host PRIVMSG #channel :Hello\r\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        let msg = transport.next().await.unwrap().unwrap();
        assert_eq!(msg.command_name(), "PRIVMSG");
        assert_eq!(msg.tag_value("time"), Some("2023-01-01"));
        assert_eq!(msg.tag_value("msgid"), Some("abc"));
        assert_eq!(msg.source_nickname(), Some("nick"));
    }

    #[tokio::test]
    async fn test_zero_copy_oversized() {
        // Create a line that exceeds the max length
        let long_line = format!("PRIVMSG #channel :{}\r\n", "A".repeat(MAX_IRC_LINE_LEN));
        let reader = MockReader::new(long_line.as_bytes());
        let mut transport = ZeroCopyTransport::new(reader);

        let result = transport.next().await;
        assert!(result.is_some());
        let unwrapped = result.unwrap();
        match unwrapped {
            Err(TransportReadError::Protocol(crate::error::ProtocolError::MessageTooLong {
                ..
            })) => {}
            other => panic!("Expected MessageTooLong error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_zero_copy_with_buffer() {
        // Simulate upgrading from Transport with buffered data
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(b"PING :buffered\r\n");

        let reader = MockReader::new(b"PING :fresh\r\n");
        let mut transport = ZeroCopyTransport::with_buffer(reader, buffer);

        // Should get buffered message first - drop before getting next
        {
            let msg1 = transport.next().await.unwrap().unwrap();
            assert_eq!(msg1.args(), &["buffered"]);
        }

        // Then fresh data
        {
            let msg2 = transport.next().await.unwrap().unwrap();
            assert_eq!(msg2.args(), &["fresh"]);
        }
    }

    #[tokio::test]
    async fn test_zero_copy_lf_only() {
        // IRC also accepts LF without CR
        let data = b"PING :server\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        let msg = transport.next().await.unwrap().unwrap();
        assert_eq!(msg.command_name(), "PING");
    }

    #[tokio::test]
    async fn test_zero_copy_invalid_utf8() {
        let data = [b'P', b'I', b'N', b'G', b' ', 0xFF, 0xFE, b'\r', b'\n'];
        let reader = MockReader::new(&data);
        let mut transport = ZeroCopyTransport::new(reader);

        let result = transport.next().await;
        assert!(result.is_some());
        let unwrapped = result.unwrap();
        match unwrapped {
            Err(TransportReadError::Protocol(crate::error::ProtocolError::InvalidUtf8 {
                ..
            })) => {
                // Expected - invalid UTF-8 sequence
            }
            Err(TransportReadError::Io(e)) if e.to_string().contains("UTF-8") => {
                // Also acceptable - IO layer caught UTF-8 error
            }
            other => panic!("Expected UTF-8 error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_zero_copy_control_char() {
        // NUL character is now allowed for binary data (e.g., METADATA values)
        let data = b"PING :server\x00test\r\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        let result = transport.next().await;
        assert!(result.is_some());
        let unwrapped = result.unwrap();
        match unwrapped {
            Ok(msg_ref) => {
                assert_eq!(msg_ref.command.name, "PING");
                assert_eq!(msg_ref.command.args[0], "server\x00test");
            }
            other => panic!("Expected Ok with NUL in content, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_zero_copy_fragmented() {
        // Simulate data arriving in small chunks
        // For this test, we use a reader that gives all data at once,
        // but we verify parsing works correctly with various message types
        let data = b":server 001 nick :Welcome\r\n:server 002 nick :Your host\r\n";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        // Process first message and drop it before calling next() again
        {
            let msg1 = transport.next().await.unwrap().unwrap();
            assert!(msg1.is_numeric());
            assert_eq!(msg1.numeric_code(), Some(1));
        }

        // Now we can safely get the next message
        {
            let msg2 = transport.next().await.unwrap().unwrap();
            assert!(msg2.is_numeric());
            assert_eq!(msg2.numeric_code(), Some(2));
        }

        assert!(transport.next().await.is_none());
    }

    #[tokio::test]
    async fn test_zero_copy_eof_incomplete() {
        // Data with no newline - should error on EOF
        let data = b"PING :incomplete";
        let reader = MockReader::new(data);
        let mut transport = ZeroCopyTransport::new(reader);

        let result = transport.next().await;
        assert!(result.is_some());
        let unwrapped = result.unwrap();
        match unwrapped {
            Err(TransportReadError::Io(e)) => {
                assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof);
            }
            other => panic!("Expected UnexpectedEof error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_into_parts_preserves_buffer() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client = async move {
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            // Send both messages in a single write so the framed reader may read
            // both and leave the second in the read buffer.
            s.write_all(b"NICK test\r\nUSER test 0 * :Test\r\n")
                .await
                .unwrap();
        };

        let server = async move {
            let (stream, _peer) = listener.accept().await.unwrap();
            let mut transport = Transport::tcp(stream).unwrap();

            let msg = transport.read_message().await.unwrap().unwrap();
            use crate::command::Command;
            match msg.command {
                Command::NICK(_) => {}
                _ => panic!("Expected NICK command"),
            }

            let parts = transport.into_parts().unwrap();
            // Ensure there is leftover data with USER
            let leftover = std::str::from_utf8(&parts.read_buf).unwrap();
            assert!(leftover.contains("USER "));
        };

        tokio::join!(client, server);
    }

    #[tokio::test]
    async fn test_upgrade_split_zero_copy() {
        use crate::command::Command;
        use crate::message::Message;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio_util::codec::FramedWrite;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client = async move {
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            s.write_all(b"NICK test\r\nUSER test 0 * :Test\r\n")
                .await
                .unwrap();

            // Read response from server (writer) - the server will send a PRIVMSG
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).await.unwrap();
            let s = std::str::from_utf8(&buf[..n]).unwrap();
            assert!(s.contains("PRIVMSG"));
        };

        let server = async move {
            let (stream, _peer) = listener.accept().await.unwrap();
            let mut transport = Transport::tcp(stream).unwrap();

            // Read first message (NICK)
            let msg = transport.read_message().await.unwrap().unwrap();
            match msg.command {
                Command::NICK(_) => {}
                _ => panic!("Expected NICK command"),
            }

            // Upgrade & split
            let parts = transport.into_parts().unwrap();
            let (read, write) = parts.split();

            // Create zero-copy reader using the read half and read buffer
            match read.half {
                TransportReadHalf::Tcp(r) => {
                    let mut zero = ZeroCopyTransport::with_buffer(r, read.read_buf);
                    // For the purposes of this test, read next message (USER)
                    let next_msg = zero.next().await.unwrap().unwrap();
                    assert!(next_msg.is_numeric() || next_msg.command_name() != "");
                }
                _ => panic!("Expected Tcp read half"),
            }

            // For writer, send a PRIVMSG to the client
            match write.half {
                TransportWriteHalf::Tcp(w) => {
                    let mut framed_write = FramedWrite::new(w, write.codec);
                    use futures_util::SinkExt;
                    framed_write
                        .send(Message::privmsg("test", "Hello from server"))
                        .await
                        .unwrap();
                }
                _ => panic!("Expected Tcp write half"),
            }
        };

        tokio::join!(client, server);
    }
}
