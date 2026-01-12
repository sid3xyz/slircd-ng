//! EXTERNAL SASL mechanism.
//!
//! Certificate-based authentication using TLS client certificates.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Encode an EXTERNAL mechanism response.
///
/// For EXTERNAL, the response is typically empty ("+") or contains
/// the authorization identity if different from the certificate CN.
///
/// # Arguments
///
/// * `authzid` - Optional authorization identity. Pass `None` for default.
pub fn encode_external(authzid: Option<&str>) -> String {
    match authzid {
        Some(id) if !id.is_empty() => BASE64.encode(id.as_bytes()),
        _ => "+".to_owned(), // Empty response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_external_empty() {
        let encoded = encode_external(None);
        assert_eq!(encoded, "+");
    }

    #[test]
    fn test_encode_external_with_authzid() {
        let encoded = encode_external(Some("myuser"));
        let decoded = BASE64.decode(&encoded).unwrap();
        assert_eq!(decoded, b"myuser");
    }
}
