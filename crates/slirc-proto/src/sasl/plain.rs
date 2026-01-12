//! PLAIN SASL mechanism (RFC 4616).
//!
//! Simple username/password authentication mechanism.
//!
//! # Reference
//! - RFC 4616: <https://tools.ietf.org/html/rfc4616>

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Encode credentials for the PLAIN mechanism.
///
/// The PLAIN mechanism encodes: `authzid NUL authcid NUL password`
///
/// For IRC SASL, `authzid` is typically empty and `authcid` is the username.
///
/// # Arguments
///
/// * `username` - The authentication identity (authcid)
/// * `password` - The password
///
/// # Returns
///
/// Base64-encoded PLAIN authentication string.
///
/// # Example
///
/// ```
/// use slirc_proto::sasl::encode_plain;
///
/// let encoded = encode_plain("testuser", "testpass");
/// // Decodes to: "\0testuser\0testpass"
/// assert!(!encoded.is_empty());
/// ```
pub fn encode_plain(username: &str, password: &str) -> String {
    // Format: authzid NUL authcid NUL password
    // For IRC, authzid is typically empty
    let payload = format!("\0{}\0{}", username, password);
    BASE64.encode(payload.as_bytes())
}

/// Encode credentials for the PLAIN mechanism with an explicit authzid.
///
/// Use this when you need to authenticate as one user but authorize as another.
///
/// # Arguments
///
/// * `authzid` - The authorization identity (who to act as)
/// * `authcid` - The authentication identity (who is authenticating)
/// * `password` - The password
pub fn encode_plain_with_authzid(authzid: &str, authcid: &str, password: &str) -> String {
    let payload = format!("{}\0{}\0{}", authzid, authcid, password);
    BASE64.encode(payload.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_plain() {
        let encoded = encode_plain("testuser", "testpass");
        let decoded = BASE64.decode(&encoded).unwrap();
        assert_eq!(decoded, b"\0testuser\0testpass");
    }

    #[test]
    fn test_encode_plain_with_authzid() {
        let encoded = encode_plain_with_authzid("admin", "testuser", "testpass");
        let decoded = BASE64.decode(&encoded).unwrap();
        assert_eq!(decoded, b"admin\0testuser\0testpass");
    }
}
