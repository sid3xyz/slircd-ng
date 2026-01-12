//! IRC message codec for tokio.
//!
//! This module provides a codec that encodes and decodes IRC [`Message`] types
//! using the tokio codec framework.

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::error;
use crate::line::LineCodec;
use crate::message::Message;

/// Tokio codec for encoding/decoding IRC messages.
///
/// Wraps [`LineCodec`] and parses lines into [`Message`] types.
pub struct IrcCodec {
    inner: LineCodec,
}

impl IrcCodec {
    /// Create a new codec with the specified encoding.
    ///
    /// # Arguments
    /// * `label` - Encoding label (e.g., "utf-8", "iso-8859-1")
    pub fn new(label: &str) -> error::Result<Self> {
        LineCodec::new(label).map(|codec| Self { inner: codec })
    }

    /// Create a new codec with custom max line length.
    ///
    /// # Arguments
    /// * `label` - Encoding label
    /// * `max_len` - Maximum line length in bytes
    pub fn with_max_len(label: &str, max_len: usize) -> error::Result<Self> {
        LineCodec::with_max_len(label, max_len).map(|codec| Self { inner: codec })
    }

    /// Sanitize outgoing message data.
    ///
    /// - Truncates at first line ending
    /// - Rejects NUL and control characters
    pub fn sanitize(mut data: String) -> error::Result<String> {
        // Truncate at first line ending
        if let Some((pos, len)) = ["\r\n", "\r", "\n"]
            .iter()
            .flat_map(|needle| data.find(needle).map(|pos| (pos, needle.len())))
            .min_by_key(|&(pos, _)| pos)
        {
            data.truncate(pos + len);
        }

        // Reject illegal control characters
        for ch in data.chars() {
            if crate::format::is_illegal_control_char(ch) {
                return Err(error::ProtocolError::IllegalControlChar(ch));
            }
        }

        Ok(data)
    }
}

impl Decoder for IrcCodec {
    type Item = Message;
    type Error = error::ProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> error::Result<Option<Message>> {
        self.inner
            .decode(src)
            .and_then(|res| res.map_or(Ok(None), |msg| msg.parse::<Message>().map(Some)))
    }
}

impl Encoder<Message> for IrcCodec {
    type Error = error::ProtocolError;

    fn encode(&mut self, msg: Message, dst: &mut BytesMut) -> error::Result<()> {
        let sanitized = Self::sanitize(msg.to_string())?;
        self.inner.encode(sanitized, dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_truncates_newline() {
        let result = IrcCodec::sanitize("PRIVMSG #test :hello\r\nworld".to_string());
        assert_eq!(result.unwrap(), "PRIVMSG #test :hello\r\n");
    }

    #[test]
    fn test_sanitize_allows_nul() {
        // NUL bytes are now allowed for binary data (e.g., METADATA values)
        let result = IrcCodec::sanitize("PRIVMSG #test :hel\0lo".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_clean() {
        let result = IrcCodec::sanitize("PRIVMSG #test :hello".to_string());
        assert_eq!(result.unwrap(), "PRIVMSG #test :hello");
    }
}
