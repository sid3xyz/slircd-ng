//! TCP and TLS zero-copy transport implementation.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::ProtocolError;
use crate::message::MessageRef;
use crate::Message;

use super::super::error::TransportReadError;
use super::super::MAX_IRC_LINE_LEN;
use super::helpers::{find_crlf, validate_irc_line_length, validate_line};
use super::trait_def::LendingStream;

/// Zero-copy transport that yields `MessageRef<'_>` without allocations.
///
/// This transport maintains an internal buffer and parses messages directly
/// from the buffer bytes, yielding borrowed `MessageRef` values that reference
/// the buffer data.
///
/// # Performance
///
/// This transport is designed for hot loops where allocations are expensive:
/// - No heap allocations per message
/// - Minimal buffer management overhead
/// - Direct parsing from byte buffer
///
/// # Usage
///
/// ```ignore
/// let mut transport = ZeroCopyTransport::new(tcp_stream);
/// while let Some(result) = transport.next().await {
///     let msg_ref = result?;
///     // Process msg_ref - it borrows from transport's buffer
/// }
/// ```
pub struct ZeroCopyTransport<S> {
    stream: S,
    buffer: BytesMut,
    consumed: usize,
    max_line_len: usize,
    /// Whether we are currently skipping bytes until a newline because of a buffer overflow
    skipping_overflow: bool,
}

impl<S> ZeroCopyTransport<S> {
    /// Create a new zero-copy transport wrapping the given stream.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: BytesMut::with_capacity(8192),
            consumed: 0,
            max_line_len: MAX_IRC_LINE_LEN,
            skipping_overflow: false,
        }
    }

    /// Create a new zero-copy transport with an existing buffer.
    ///
    /// This is useful when upgrading from a `Transport` that has buffered
    /// data that hasn't been processed yet.
    pub fn with_buffer(stream: S, buffer: BytesMut) -> Self {
        Self {
            stream,
            buffer,
            consumed: 0,
            max_line_len: MAX_IRC_LINE_LEN,
            skipping_overflow: false,
        }
    }

    /// Create a new zero-copy transport with a custom maximum line length.
    pub fn with_max_line_len(stream: S, max_len: usize) -> Self {
        Self {
            stream,
            buffer: BytesMut::with_capacity(max_len.min(65536)),
            consumed: 0,
            max_line_len: max_len,
            skipping_overflow: false,
        }
    }

    /// Set the maximum line length.
    pub fn set_max_line_len(&mut self, len: usize) {
        self.max_line_len = len;
    }

    /// Consume this transport and return its inner stream and buffer.
    ///
    /// This is useful for STARTTLS upgrade: extract the TCP stream,
    /// perform TLS handshake, then create a new transport with the
    /// TLS stream and preserved buffer.
    ///
    /// # Returns
    ///
    /// A tuple of `(stream, buffer)` where buffer contains any unprocessed data.
    pub fn into_parts(mut self) -> (S, BytesMut) {
        // Advance past consumed data before returning
        if self.consumed > 0 {
            self.buffer.advance(self.consumed);
        }
        (self.stream, self.buffer)
    }
}

impl<S: AsyncWrite + Unpin> ZeroCopyTransport<S> {
    /// Write an IRC message to the transport.
    ///
    /// This serializes the message with CRLF terminator and writes it
    /// to the underlying stream, then flushes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// transport.write_message(&Message::pong("server")).await?;
    /// ```
    pub async fn write_message(&mut self, message: &Message) -> std::io::Result<()> {
        let serialized = message.to_string();
        self.stream.write_all(serialized.as_bytes()).await?;
        self.stream.flush().await
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
    /// // Forward message from one server to another
    /// let msg_ref = incoming_transport.next().await?;
    /// outgoing_transport.write_message_ref(&msg_ref).await?;
    /// ```
    pub async fn write_message_ref(&mut self, message: &MessageRef<'_>) -> std::io::Result<()> {
        use std::fmt::Write;
        // Use a small stack buffer for typical messages, heap-allocate only if needed
        let mut buf = String::with_capacity(512);
        write!(&mut buf, "{}", message).expect("fmt::Write to String cannot fail");
        self.stream.write_all(buf.as_bytes()).await?;
        self.stream.flush().await
    }
}

impl<S: AsyncRead + Unpin> ZeroCopyTransport<S> {
    /// Read the next message from the transport.
    ///
    /// Returns `None` when the stream is closed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// while let Some(result) = transport.next().await {
    ///     let msg_ref = result?;
    ///     println!("Command: {}", msg_ref.command_name());
    /// }
    /// ```
    pub async fn next(&mut self) -> Option<Result<MessageRef<'_>, TransportReadError>> {
        // Advance past any previously consumed data
        if self.consumed > 0 {
            let consumed = self.consumed;
            self.buffer.advance(consumed);
            self.consumed = 0;
        }

        loop {
            // If we are in skipping mode, look for a newline to recover
            if self.skipping_overflow {
                if let Some(newline_pos) = find_crlf(&self.buffer) {
                    // Found the end of the garbage line
                    let line_len = newline_pos + 1;
                    self.consumed = line_len;
                    self.skipping_overflow = false;

                    // We found the end of the bad line. We can now consume it and continue
                    // to try to read the NEXT line in the same loop iteration.
                    // We must advance manually here because we are 'continue'ing the loop
                    // and bypassing the top-of-loop advance check (which only runs if we return).
                    self.buffer.advance(self.consumed);
                    self.consumed = 0;
                    continue;
                } else {
                    // No newline yet, discard everything to prevent buffer exhaustion
                    let len = self.buffer.len();
                    self.buffer.advance(len);
                    // Keep skipping_overflow = true, read more data
                }
            } else {
                // Check if we have a complete line in the buffer
                if let Some(newline_pos) = find_crlf(&self.buffer) {
                    let line_len = newline_pos + 1;

                    // Validate the line slice for UTF-8 first
                    let line_slice = &self.buffer[..line_len];

                    // Validate IRC-specific line lengths (tags vs body)
                    // This checks:
                    // - Client tag data ≤ 4094 bytes
                    // - Message body ≤ max_line_len bytes (including CRLF)
                    if let Err(e) = validate_irc_line_length(line_slice, self.max_line_len) {
                        // Mark the line as consumed so we can continue reading
                        self.consumed = line_len;
                        return Some(Err(e));
                    }

                    // Validate UTF-8 and control characters
                    match validate_line(line_slice) {
                        Ok(line_str) => {
                            // Mark this line as consumed (will be advanced on next call)
                            self.consumed = line_len;

                            // Parse the message - no unsafe needed here because:
                            // - The `&mut self` borrow prevents calling `next()` again while MessageRef is live
                            // - Buffer advancement is deferred until the next call to `next()`
                            // - The returned MessageRef lifetime is tied to `self` via function signature
                            match MessageRef::parse(line_str) {
                                Ok(msg) => return Some(Ok(msg)),
                                Err(e) => {
                                    return Some(Err(TransportReadError::Protocol(
                                        ProtocolError::InvalidMessage {
                                            string: line_str.to_string(),
                                            cause: e,
                                        },
                                    )))
                                }
                            }
                        }
                        Err(e) => {
                            // CRITICAL: Consume the line even on UTF-8 failure
                            // to prevent infinite loop on same invalid bytes
                            self.consumed = line_len;
                            return Some(Err(e));
                        }
                    }
                }

                // Check if buffer is getting too large without a complete line
                if self.buffer.len() > self.max_line_len {
                    self.skipping_overflow = true;
                    return Some(Err(TransportReadError::Protocol(
                        ProtocolError::MessageTooLong {
                            actual: self.buffer.len(),
                            limit: self.max_line_len,
                        },
                    )));
                }
            }

            // Need more data - read from stream
            let mut temp = [0u8; 4096];
            match self.stream.read(&mut temp).await {
                Ok(0) => {
                    // EOF - stream closed
                    if self.buffer.is_empty() {
                        return None;
                    } else {
                        // Incomplete message at EOF
                        return Some(Err(TransportReadError::Io(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "Stream closed with incomplete message",
                        ))));
                    }
                }
                Ok(n) => {
                    self.buffer.extend_from_slice(&temp[..n]);
                }
                Err(e) => return Some(Err(TransportReadError::Io(e))),
            }
        }
    }
}

impl<S: AsyncRead + Unpin> LendingStream for ZeroCopyTransport<S> {
    type Item<'a>
        = MessageRef<'a>
    where
        Self: 'a;
    type Error = TransportReadError;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item<'_>, Self::Error>>> {
        // Advance past any previously consumed data
        if self.consumed > 0 {
            let consumed = self.consumed;
            self.buffer.advance(consumed);
            self.consumed = 0;
        }

        loop {
            // Check if we have a complete line in the buffer
            if let Some(newline_pos) = find_crlf(&self.buffer) {
                let line_len = newline_pos + 1;

                // Check line length limit
                if line_len > self.max_line_len {
                    return Poll::Ready(Some(Err(TransportReadError::Protocol(
                        ProtocolError::MessageTooLong {
                            actual: line_len,
                            limit: self.max_line_len,
                        },
                    ))));
                }

                // Validate the line first (this borrows buffer temporarily)
                {
                    let line_slice = &self.buffer[..line_len];
                    if let Err(e) = validate_line(line_slice) {
                        return Poll::Ready(Some(Err(e)));
                    }
                }

                // Mark this line as consumed and get long-lived reference
                // We use get_mut() to get a &'a mut Self from Pin<&'a mut Self>
                // This allows us to return a reference tied to 'a.
                let this = self.get_mut();
                this.consumed = line_len;

                let line_str: &str = {
                    let slice = &this.buffer[..line_len];
                    std::str::from_utf8(slice).expect("Already validated as UTF-8")
                };

                match MessageRef::parse(line_str) {
                    Ok(msg) => return Poll::Ready(Some(Ok(msg))),
                    Err(e) => {
                        return Poll::Ready(Some(Err(TransportReadError::Protocol(
                            ProtocolError::InvalidMessage {
                                string: line_str.to_string(),
                                cause: e,
                            },
                        ))))
                    }
                }
            }

            // Check if buffer is getting too large
            if self.buffer.len() > self.max_line_len {
                return Poll::Ready(Some(Err(TransportReadError::Protocol(
                    ProtocolError::MessageTooLong {
                        actual: self.buffer.len(),
                        limit: self.max_line_len,
                    },
                ))));
            }

            // Need more data - try to read from stream
            let this = self.as_mut().get_mut();
            let mut read_buf = [0u8; 4096];
            let mut read_buf_slice = tokio::io::ReadBuf::new(&mut read_buf);

            match Pin::new(&mut this.stream).poll_read(cx, &mut read_buf_slice) {
                Poll::Ready(Ok(())) => {
                    let n = read_buf_slice.filled().len();
                    if n == 0 {
                        // EOF
                        if this.buffer.is_empty() {
                            return Poll::Ready(None);
                        } else {
                            return Poll::Ready(Some(Err(TransportReadError::Io(
                                std::io::Error::new(
                                    std::io::ErrorKind::UnexpectedEof,
                                    "Stream closed with incomplete message",
                                ),
                            ))));
                        }
                    }
                    this.buffer.extend_from_slice(read_buf_slice.filled());
                    // Loop to check buffer again
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(TransportReadError::Io(e)))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
