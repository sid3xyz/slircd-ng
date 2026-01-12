//! Non-IRC protocol detection.
//!
//! This module provides utilities to detect when a client is speaking
//! a protocol other than IRC (e.g., HTTP, SMTP, SSH, TLS).
//!
//! This is useful for IRC servers that need to reject or redirect
//! connections from clients using the wrong protocol.

/// Detected protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetectedProtocol {
    /// Internet Relay Chat (expected)
    Irc,
    /// HTTP/HTTPS request
    Http,
    /// SMTP mail protocol
    Smtp,
    /// SSH connection
    Ssh,
    /// TLS/SSL handshake (raw TLS on plaintext port)
    Tls,
    /// Telnet negotiation
    Telnet,
    /// Unknown non-IRC protocol
    Unknown,
}

impl DetectedProtocol {
    /// Returns the protocol name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Irc => "IRC",
            Self::Http => "HTTP",
            Self::Smtp => "SMTP",
            Self::Ssh => "SSH",
            Self::Tls => "TLS/SSL",
            Self::Telnet => "Telnet",
            Self::Unknown => "Unknown",
        }
    }

    /// Returns true if this is a non-IRC protocol.
    pub fn is_non_irc(&self) -> bool {
        !matches!(self, Self::Irc)
    }
}

/// HTTP method prefixes.
const HTTP_METHODS: &[&str] = &[
    "GET ", "POST ", "PUT ", "DELETE ", "HEAD ", "OPTIONS ", "PATCH ", "CONNECT ", "TRACE ",
];

/// SMTP command prefixes.
const SMTP_COMMANDS: &[&str] = &["HELO ", "EHLO ", "MAIL ", "RCPT ", "DATA "];

/// Detect the protocol from the first line of input.
///
/// # Example
///
/// ```
/// use slirc_proto::scanner::{detect_protocol, DetectedProtocol};
///
/// assert_eq!(detect_protocol("NICK foo"), DetectedProtocol::Irc);
/// assert_eq!(detect_protocol("GET / HTTP/1.1"), DetectedProtocol::Http);
/// assert_eq!(detect_protocol("SSH-2.0-OpenSSH"), DetectedProtocol::Ssh);
/// ```
pub fn detect_protocol(line: &str) -> DetectedProtocol {
    if line.is_empty() {
        return DetectedProtocol::Irc; // Empty line, assume IRC
    }

    // Check for SSH
    if line.starts_with("SSH-") {
        return DetectedProtocol::Ssh;
    }

    // Check for TLS ClientHello (0x16 = handshake record type)
    if line.as_bytes()[0] == 0x16 {
        return DetectedProtocol::Tls;
    }

    // Check for HTTP methods
    if HTTP_METHODS.iter().any(|method| line.starts_with(method)) {
        return DetectedProtocol::Http;
    }

    // Check for SMTP commands
    if SMTP_COMMANDS.iter().any(|cmd| line.starts_with(cmd)) {
        return DetectedProtocol::Smtp;
    }

    // Check for Telnet negotiation (IAC = 0xFF, or replacement char from bad encoding)
    if let Some(first_char) = line.chars().next() {
        if first_char == '\u{FFFD}' || line.as_bytes()[0] == 0xFF {
            return DetectedProtocol::Telnet;
        }
    }

    // Assume IRC if no other protocol detected
    DetectedProtocol::Irc
}

/// Check if a line appears to be a non-IRC protocol.
///
/// This is a convenience wrapper around [`detect_protocol`].
///
/// # Example
///
/// ```
/// use slirc_proto::scanner::is_non_irc_protocol;
///
/// assert!(!is_non_irc_protocol("NICK foo"));
/// assert!(is_non_irc_protocol("GET / HTTP/1.1"));
/// ```
#[inline]
pub fn is_non_irc_protocol(line: &str) -> bool {
    detect_protocol(line).is_non_irc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_irc() {
        assert_eq!(detect_protocol("NICK foo"), DetectedProtocol::Irc);
        assert_eq!(
            detect_protocol("USER guest 0 * :Real"),
            DetectedProtocol::Irc
        );
        assert_eq!(detect_protocol("PING :server"), DetectedProtocol::Irc);
        assert_eq!(detect_protocol(""), DetectedProtocol::Irc);
    }

    #[test]
    fn test_detect_http() {
        assert_eq!(detect_protocol("GET / HTTP/1.1"), DetectedProtocol::Http);
        assert_eq!(
            detect_protocol("POST /api HTTP/1.1"),
            DetectedProtocol::Http
        );
        assert_eq!(
            detect_protocol("HEAD /index.html HTTP/1.0"),
            DetectedProtocol::Http
        );
    }

    #[test]
    fn test_detect_smtp() {
        assert_eq!(detect_protocol("HELO example.com"), DetectedProtocol::Smtp);
        assert_eq!(
            detect_protocol("EHLO mail.server.com"),
            DetectedProtocol::Smtp
        );
    }

    #[test]
    fn test_detect_ssh() {
        assert_eq!(
            detect_protocol("SSH-2.0-OpenSSH_8.0"),
            DetectedProtocol::Ssh
        );
        assert_eq!(detect_protocol("SSH-1.99-PuTTY"), DetectedProtocol::Ssh);
    }

    #[test]
    fn test_detect_tls() {
        // 0x16 is TLS handshake record type
        assert_eq!(detect_protocol("\x16\x03\x01"), DetectedProtocol::Tls);
    }

    #[test]
    fn test_detect_telnet() {
        // 0xFF is Telnet IAC - use byte string
        let telnet_data = String::from_utf8_lossy(&[0xFF, 0xFD, 0x18]);
        assert_eq!(detect_protocol(&telnet_data), DetectedProtocol::Telnet);
    }

    #[test]
    fn test_is_non_irc() {
        assert!(!is_non_irc_protocol("NICK foo"));
        assert!(is_non_irc_protocol("GET / HTTP/1.1"));
        assert!(is_non_irc_protocol("SSH-2.0-OpenSSH"));
    }

    #[test]
    fn test_protocol_as_str() {
        assert_eq!(DetectedProtocol::Irc.as_str(), "IRC");
        assert_eq!(DetectedProtocol::Http.as_str(), "HTTP");
        assert_eq!(DetectedProtocol::Ssh.as_str(), "SSH");
    }
}
