//! WebSocket zero-copy transport implementation.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, BytesMut};
use futures_util::{SinkExt, Stream, StreamExt};
use tokio_tungstenite::{tungstenite::Message as WsMessage, WebSocketStream};

use crate::error::ProtocolError;
use crate::message::MessageRef;
use crate::Message;

use super::super::error::TransportReadError;
use super::super::MAX_IRC_LINE_LEN;
use super::helpers::{find_crlf, validate_irc_line_length, validate_line};
use super::trait_def::LendingStream;

/// Zero-copy transport wrapper for WebSocket streams.
///
/// WebSocket uses frame-based messaging rather than byte streaming, so this
/// wrapper extracts text payloads from frames and writes them to an internal
/// buffer for zero-copy parsing.
pub struct ZeroCopyWebSocketTransport<S> {
    stream: WebSocketStream<S>,
    buffer: BytesMut,
    consumed: usize,
    max_line_len: usize,
}

impl<S> ZeroCopyWebSocketTransport<S> {
    /// Create a new zero-copy WebSocket transport.
    pub fn new(stream: WebSocketStream<S>) -> Self {
        Self {
            stream,
            buffer: BytesMut::with_capacity(8192),
            consumed: 0,
            max_line_len: MAX_IRC_LINE_LEN,
        }
    }

    /// Create with an existing buffer (for upgrade from Transport).
    pub fn with_buffer(stream: WebSocketStream<S>, buffer: BytesMut) -> Self {
        Self {
            stream,
            buffer,
            consumed: 0,
            max_line_len: MAX_IRC_LINE_LEN,
        }
    }

    /// Set the maximum line length.
    pub fn set_max_line_len(&mut self, len: usize) {
        self.max_line_len = len;
    }
}

impl<S> ZeroCopyWebSocketTransport<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    /// Read the next message from the WebSocket transport.
    pub async fn next(&mut self) -> Option<Result<MessageRef<'_>, TransportReadError>> {
        if self.consumed > 0 {
            let consumed = self.consumed;
            self.buffer.advance(consumed);
            self.consumed = 0;
        }

        loop {
            // Check if we have a complete line in the buffer
            if let Some(newline_pos) = find_crlf(&self.buffer) {
                let line_len = newline_pos + 1;

                let line_slice = &self.buffer[..line_len];

                // Validate IRC-specific line lengths (tags vs body)
                if let Err(e) = validate_irc_line_length(line_slice, self.max_line_len) {
                    self.consumed = line_len;
                    return Some(Err(e));
                }

                match validate_line(line_slice) {
                    Ok(line_str) => {
                        self.consumed = line_len;

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
                    Err(e) => return Some(Err(e)),
                }
            }

            // Check buffer size limit
            if self.buffer.len() > self.max_line_len {
                return Some(Err(TransportReadError::Protocol(
                    ProtocolError::MessageTooLong {
                        actual: self.buffer.len(),
                        limit: self.max_line_len,
                    },
                )));
            }

            // Need more data - read from WebSocket
            match self.stream.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    // WebSocket IRC messages may or may not have CRLF
                    // Append the text, ensuring it ends with LF for our line parser
                    let text = text.trim_end_matches(['\r', '\n']);
                    self.buffer.extend_from_slice(text.as_bytes());
                    self.buffer.extend_from_slice(b"\n");
                }
                Some(Ok(WsMessage::Close(_))) | None => {
                    if self.buffer.is_empty() {
                        return None;
                    } else {
                        return Some(Err(TransportReadError::Io(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "WebSocket closed with incomplete message",
                        ))));
                    }
                }
                Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_))) => {
                    // Ignore control frames, continue reading
                    continue;
                }
                Some(Ok(WsMessage::Binary(_))) => {
                    // IRC is text-only, skip binary frames
                    continue;
                }
                Some(Err(e)) => {
                    return Some(Err(TransportReadError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("WebSocket error: {}", e),
                    ))));
                }
            }
        }
    }

    /// Write an IRC message to the WebSocket transport.
    ///
    /// This sends the message as a WebSocket text frame. The CRLF
    /// terminator is stripped since WebSocket uses frame boundaries.
    pub async fn write_message(&mut self, message: &Message) -> std::io::Result<()> {
        let text = message.to_string();
        let text = text.trim_end_matches(&['\r', '\n'][..]);
        self.stream
            .send(WsMessage::Text(text.to_string()))
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    /// Write a borrowed IRC message to the WebSocket transport (zero-copy forwarding).
    ///
    /// This is optimized for relay scenarios where you receive a `MessageRef`
    /// and want to forward it without allocating an owned `Message`.
    pub async fn write_message_ref(&mut self, message: &MessageRef<'_>) -> std::io::Result<()> {
        use std::fmt::Write;
        let mut buf = String::with_capacity(512);
        write!(&mut buf, "{}", message).expect("fmt::Write to String cannot fail");
        // Strip CRLF for WebSocket (uses frame boundaries)
        let text = buf.trim_end_matches(&['\r', '\n'][..]);
        self.stream
            .send(WsMessage::Text(text.to_string()))
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

impl<S> LendingStream for ZeroCopyWebSocketTransport<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    type Item<'a>
        = MessageRef<'a>
    where
        Self: 'a;
    type Error = TransportReadError;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item<'_>, Self::Error>>> {
        if self.consumed > 0 {
            let consumed = self.consumed;
            self.buffer.advance(consumed);
            self.consumed = 0;
        }

        loop {
            // Check if we have a complete line in the buffer
            if let Some(newline_pos) = find_crlf(&self.buffer) {
                let line_len = newline_pos + 1;

                if line_len > self.max_line_len {
                    return Poll::Ready(Some(Err(TransportReadError::Protocol(
                        ProtocolError::MessageTooLong {
                            actual: line_len,
                            limit: self.max_line_len,
                        },
                    ))));
                }

                // Validate line first
                {
                    let line_slice = &self.buffer[..line_len];
                    if let Err(e) = validate_line(line_slice) {
                        return Poll::Ready(Some(Err(e)));
                    }
                }

                // Mark this line as consumed and get long-lived reference
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

            // Check buffer size limit
            if self.buffer.len() > self.max_line_len {
                return Poll::Ready(Some(Err(TransportReadError::Protocol(
                    ProtocolError::MessageTooLong {
                        actual: self.buffer.len(),
                        limit: self.max_line_len,
                    },
                ))));
            }

            // Need more data - poll WebSocket
            let this = self.as_mut().get_mut();
            match Pin::new(&mut this.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(WsMessage::Text(text)))) => {
                    let text = text.trim_end_matches(['\r', '\n']);
                    this.buffer.extend_from_slice(text.as_bytes());
                    this.buffer.extend_from_slice(b"\n");
                    // Loop to check buffer again
                }
                Poll::Ready(Some(Ok(WsMessage::Close(_)))) | Poll::Ready(None) => {
                    if this.buffer.is_empty() {
                        return Poll::Ready(None);
                    } else {
                        return Poll::Ready(Some(Err(TransportReadError::Io(
                            std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "WebSocket closed with incomplete message",
                            ),
                        ))));
                    }
                }
                Poll::Ready(Some(Ok(
                    WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_),
                ))) => {
                    continue;
                }
                Poll::Ready(Some(Ok(WsMessage::Binary(_)))) => {
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(TransportReadError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("WebSocket error: {}", e),
                    )))));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
