//! Line-based codec for tokio.
//!
//! This module provides a codec that reads/writes newline-terminated lines,
//! with optional encoding support.

#[cfg(feature = "encoding")]
use std::borrow::Cow;
#[cfg(feature = "encoding")]
use std::io;

use bytes::BytesMut;
#[cfg(feature = "encoding")]
use encoding::Encoding;
use tokio_util::codec::{Decoder, Encoder};

use crate::error;

/// Line-based codec that handles newline-terminated messages.
///
/// By default, lines are limited to 512 bytes (IRC standard).
pub struct LineCodec {
    #[cfg(feature = "encoding")]
    encoding: &'static Encoding,
    /// Index of next byte to check for newline
    next_index: usize,
    /// Maximum line length
    max_len: usize,
}

impl LineCodec {
    /// Create a new codec with the specified encoding.
    ///
    /// # Arguments
    /// * `label` - Encoding label (e.g., "utf-8")
    pub fn new(_label: &str) -> error::Result<Self> {
        Ok(Self {
            #[cfg(feature = "encoding")]
            encoding: match Encoding::for_label(_label.as_bytes()) {
                Some(enc) => enc,
                None => {
                    return Err(error::ProtocolError::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("Unknown encoding: {}", _label),
                    )));
                }
            },
            next_index: 0,
            max_len: 512,
        })
    }

    /// Create a new codec with custom max line length.
    pub fn with_max_len(label: &str, max_len: usize) -> error::Result<Self> {
        let mut codec = Self::new(label)?;
        codec.max_len = max_len;
        Ok(codec)
    }

    /// Validate that a string contains no illegal control characters.
    fn validate_line(s: &str) -> error::Result<()> {
        let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
        for ch in trimmed.chars() {
            if crate::format::is_illegal_control_char(ch) {
                return Err(error::ProtocolError::IllegalControlChar(ch));
            }
        }
        Ok(())
    }
}

impl Decoder for LineCodec {
    type Item = String;
    type Error = error::ProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> error::Result<Option<String>> {
        // Look for newline starting from where we left off
        if let Some(offset) = src[self.next_index..].iter().position(|b| *b == b'\n') {
            // Found a line - extract it
            let line = src.split_to(self.next_index + offset + 1);
            self.next_index = 0;

            // Check length limit
            if line.len() > self.max_len {
                return Err(error::ProtocolError::MessageTooLong {
                    actual: line.len(),
                    limit: self.max_len,
                });
            }

            // Decode bytes to string
            #[cfg(feature = "encoding")]
            let data = {
                let (cow, _enc, _had_errors) = self.encoding.decode(line.as_ref());
                cow.into_owned()
            };

            #[cfg(not(feature = "encoding"))]
            let data = {
                let line_vec = line.to_vec();
                String::from_utf8(line_vec.clone()).map_err(|e| {
                    error::ProtocolError::InvalidUtf8 {
                        raw_line: line_vec,
                        byte_pos: e.utf8_error().valid_up_to(),
                        details: e.utf8_error().to_string(),
                        command_hint: error::extract_command_hint(&line),
                    }
                })?
            };

            // Validate no illegal control characters
            Self::validate_line(&data)?;

            Ok(Some(data))
        } else {
            // No complete line yet - remember where we stopped
            self.next_index = src.len();

            // Check if partial line already exceeds limit
            if src.len() > self.max_len {
                return Err(error::ProtocolError::MessageTooLong {
                    actual: src.len(),
                    limit: self.max_len,
                });
            }

            Ok(None)
        }
    }
}

impl Encoder<String> for LineCodec {
    type Error = error::ProtocolError;

    fn encode(&mut self, msg: String, dst: &mut BytesMut) -> error::Result<()> {
        #[cfg(feature = "encoding")]
        {
            let (cow_bytes, _enc, _had_errors) = self.encoding.encode(&msg);
            match cow_bytes {
                Cow::Borrowed(b) => dst.extend_from_slice(b),
                Cow::Owned(v) => dst.extend_from_slice(&v),
            }
        }

        #[cfg(not(feature = "encoding"))]
        {
            dst.extend(msg.into_bytes());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_complete_line() {
        let mut codec = LineCodec::new("utf-8").unwrap();
        let mut buf = BytesMut::from("PING :test\r\n");

        let result = codec.decode(&mut buf).unwrap();
        assert_eq!(result, Some("PING :test\r\n".to_string()));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_decode_partial_line() {
        let mut codec = LineCodec::new("utf-8").unwrap();
        let mut buf = BytesMut::from("PING :");

        let result = codec.decode(&mut buf).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_decode_too_long() {
        let mut codec = LineCodec::with_max_len("utf-8", 10).unwrap();
        let mut buf = BytesMut::from("this is way too long\n");

        let result = codec.decode(&mut buf);
        assert!(matches!(
            result,
            Err(error::ProtocolError::MessageTooLong { .. })
        ));
    }

    #[test]
    fn test_encode() {
        let mut codec = LineCodec::new("utf-8").unwrap();
        let mut buf = BytesMut::new();

        codec
            .encode("PONG :test\r\n".to_string(), &mut buf)
            .unwrap();
        assert_eq!(&buf[..], b"PONG :test\r\n");
    }
}
