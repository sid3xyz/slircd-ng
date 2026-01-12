//! Transport error types.

use thiserror::Error;

use crate::error::ProtocolError;

/// Errors that can occur when reading from a transport.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TransportReadError {
    /// An I/O error occurred.
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A protocol error occurred.
    #[error("transport protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_conversion() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let transport_err: TransportReadError = io_err.into();

        match transport_err {
            TransportReadError::Io(_) => {} // Expected
            _ => panic!("Expected Io variant"),
        }

        assert_eq!(
            transport_err.to_string(),
            "transport I/O error: connection refused"
        );
    }

    #[test]
    fn test_protocol_error_conversion() {
        let protocol_err = ProtocolError::MessageTooLong {
            actual: 1024,
            limit: 512,
        };
        let transport_err: TransportReadError = protocol_err.into();

        match transport_err {
            TransportReadError::Protocol(_) => {} // Expected
            _ => panic!("Expected Protocol variant"),
        }

        assert!(transport_err
            .to_string()
            .contains("transport protocol error"));
        assert!(transport_err.to_string().contains("message too long"));
    }

    #[test]
    fn test_error_source_chaining() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe");
        let transport_err: TransportReadError = io_err.into();

        // Verify error source is properly chained
        let source = std::error::Error::source(&transport_err);
        assert!(source.is_some());
        assert_eq!(source.unwrap().to_string(), "broken pipe");
    }

    #[test]
    fn test_error_display() {
        // Test I/O error display
        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let transport_err: TransportReadError = io_err.into();
        assert_eq!(transport_err.to_string(), "transport I/O error: timed out");

        // Test Protocol error display
        let protocol_err = ProtocolError::InvalidUtf8 {
            raw_line: vec![0xff, 0xfe],
            byte_pos: 0,
            details: "invalid UTF-8 sequence".to_string(),
            command_hint: None,
        };
        let transport_err: TransportReadError = protocol_err.into();
        assert!(transport_err
            .to_string()
            .contains("invalid UTF-8 in message"));
    }
}
