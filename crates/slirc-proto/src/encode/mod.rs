//! Zero-copy encoding for IRC messages.
//!
//! This module provides the [`IrcEncode`] trait for writing IRC messages directly
//! to byte buffers without intermediate `String` allocations.
//!
//! # Design
//!
//! The standard `Display` trait formats to a `String`, which requires allocation.
//! For high-performance servers handling thousands of messages per second,
//! `IrcEncode` writes directly to any `Write` implementor (sockets, `BytesMut`, etc.).
//!
//! # Example
//!
//! ```
//! use slirc_proto::encode::IrcEncode;
//! use slirc_proto::Message;
//!
//! let msg = Message::privmsg("#channel", "Hello!");
//! let mut buf = Vec::new();
//! msg.encode(&mut buf).unwrap();
//!
//! assert_eq!(&buf, b"PRIVMSG #channel :Hello!\r\n");
//! ```

use std::io::{self, Write};

mod command;
mod message;

/// A trait for encoding IRC protocol elements directly to a byte stream.
///
/// This provides zero-copy encoding by writing directly to any [`Write`]
/// implementor, avoiding the intermediate `String` allocation that `Display` requires.
///
/// # Implementors
///
/// - [`Message`](crate::Message) - Owned IRC message
/// - [`MessageRef`](crate::MessageRef) - Borrowed IRC message
/// - [`Command`](crate::command::Command) - IRC command
/// - [`Prefix`](crate::prefix::Prefix) - Message source/prefix
pub trait IrcEncode {
    /// Encode this value to the given writer.
    ///
    /// Returns the number of bytes written on success.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the write fails.
    fn encode<W: Write>(&self, writer: &mut W) -> io::Result<usize>;

    /// Encode this value to a new `Vec<u8>`.
    ///
    /// This is a convenience method for cases where you need a buffer.
    /// For optimal performance, prefer [`encode`](Self::encode) with a pre-allocated buffer.
    #[must_use]
    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512); // IRC max line length
        let _ = self.encode(&mut buf);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::prefix::Prefix;

    #[test]
    fn test_encode_privmsg() {
        let msg = Message::privmsg("#channel", "Hello world!");
        let bytes = msg.to_bytes();
        assert_eq!(&bytes, b"PRIVMSG #channel :Hello world!\r\n");
    }

    #[test]
    fn test_encode_simple_command() {
        let msg = Message::nick("testnick");
        let bytes = msg.to_bytes();
        assert_eq!(&bytes, b"NICK testnick\r\n");
    }

    #[test]
    fn test_encode_with_prefix() {
        let msg =
            Message::privmsg("#test", "Hello").with_prefix(Prefix::new_from_str("nick!user@host"));
        let bytes = msg.to_bytes();
        assert_eq!(&bytes, b":nick!user@host PRIVMSG #test :Hello\r\n");
    }

    #[test]
    fn test_encode_with_tags() {
        let msg = Message::privmsg("#test", "Hi").with_tag("time", Some("2023-01-01T00:00:00Z"));
        let bytes = msg.to_bytes();
        assert_eq!(&bytes, b"@time=2023-01-01T00:00:00Z PRIVMSG #test :Hi\r\n");
    }

    #[test]
    fn test_encode_returns_byte_count() {
        let msg = Message::ping("server");
        let mut buf = Vec::new();
        let written = msg.encode(&mut buf).unwrap();
        assert_eq!(written, buf.len());
    }
}
