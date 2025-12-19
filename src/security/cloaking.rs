//! Hostname cloaking (masking) - Privacy for IRC users.
//!
//! Provides HMAC-SHA256 based cloaking for IP addresses and hostnames,
//! protecting user privacy while maintaining deterministic, reversible-only-by-server
//! mappings.
//!
//! # Security Model
//!
//! - Uses HMAC-SHA256 with a server-configured secret key
//! - CIDR masking before hashing preserves network structure for abuse handling
//! - Base32 encoding produces compact, URL-safe cloaked hostnames
//! - Deterministic: same IP + key always produces same cloak
//!
//! # Format Examples
//!
//! - IPv4: `abc123.def456.ghi789.ip` (3 segments from HMAC)
//! - IPv6: `abc123:def456:ghi789:ip` (colon-separated)
//! - Hostname: `abc123def456.tld` (HMAC hash + preserved TLD)

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

type HmacSha256 = Hmac<Sha256>;

/// Base32 alphabet (RFC 4648 without padding, lowercase for IRC).
const BASE32_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Encode bytes to base32 (RFC 4648 style, lowercase, no padding).
///
/// More compact than hex (1.6 bytes per char vs 2 bytes per char).
fn base32_encode(data: &[u8]) -> String {
    // Each 5 bits becomes one char, so ceil(len * 8 / 5) chars needed
    let capacity = (data.len() * 8).div_ceil(5);
    let mut result = String::with_capacity(capacity);
    let mut bits = 0u32;
    let mut bit_count = 0u8;

    for &byte in data {
        bits = (bits << 8) | (byte as u32);
        bit_count += 8;

        while bit_count >= 5 {
            bit_count -= 5;
            let index = ((bits >> bit_count) & 0x1F) as usize;
            result.push(BASE32_ALPHABET[index] as char);
        }
    }

    // Flush remaining bits
    if bit_count > 0 {
        let index = ((bits << (5 - bit_count)) & 0x1F) as usize;
        result.push(BASE32_ALPHABET[index] as char);
    }

    result
}

/// Apply CIDR mask to IP address before hashing.
///
/// Preserves network structure: /24 for IPv4, /48 for IPv6.
/// Allows network administrators to identify general origin while protecting exact IP.
fn apply_cidr_mask(ip: &IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            // /24 mask: keep first 3 octets, zero last octet
            let octets = v4.octets();
            IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], 0))
        }
        IpAddr::V6(v6) => {
            // /48 mask: keep first 6 bytes (48 bits), zero remaining 10 bytes
            let segments = v6.segments();
            IpAddr::V6(Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                0,
                0,
                0,
                0,
                0,
            ))
        }
    }
}

/// Generate hierarchical cloaked hostname from IP using HMAC-SHA256.
///
/// # Security
///
/// - Uses HMAC-SHA256 with server secret key (not reversible without key)
/// - CIDR masking before hashing preserves network structure
/// - Deterministic: same IP + key always produces same cloak
///
/// # Format
///
/// - IPv4 /24: `abc123.def456.ghi789.ip` (3 segments, ~5 chars each)
/// - IPv6 /48: `abc123:def456:ghi789:ip` (3 segments, colon-separated)
///
/// # Arguments
///
/// * `ip` - IP address to cloak
/// * `secret_key` - Server secret key for HMAC (MUST be kept private)
///
/// # Panics
///
/// Never panics - HMAC can accept keys of any size.
///
/// Generate cloaked hostname with custom suffix (e.g., "ip" for `abc123.def456.ghi789.ip`).
pub fn cloak_ip_hmac_with_suffix(ip: &IpAddr, secret_key: &str, suffix: &str) -> String {
    // Step 1: Apply CIDR mask (/24 for IPv4, /48 for IPv6)
    let masked_ip = apply_cidr_mask(ip);

    // Step 2: HMAC-SHA256 the masked IP
    // SAFETY: HMAC-SHA256 accepts keys of any length per RFC 2104, this cannot fail
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(masked_ip.to_string().as_bytes());
    let result = mac.finalize();
    let hash_bytes = result.into_bytes();

    // Step 3: Generate hierarchical segments using base32 encoding
    // Take 9 bytes (3 segments * 3 bytes each), encode to base32
    let segment1 = base32_encode(&hash_bytes[0..3]);
    let segment2 = base32_encode(&hash_bytes[3..6]);
    let segment3 = base32_encode(&hash_bytes[6..9]);

    // Step 4: Format based on IP version
    match ip {
        IpAddr::V4(_) => format!("{}.{}.{}.{}", segment1, segment2, segment3, suffix),
        IpAddr::V6(_) => format!("{}:{}:{}:{}", segment1, segment2, segment3, suffix),
    }
}

/// Cloak a hostname using HMAC-SHA256 (preserves TLD for readability).
///
/// # Format
///
/// - Input: "user.example.com"
/// - Output: "abc123def456.com" (HMAC hash + preserved TLD)
///
/// # Security
///
/// - Uses HMAC-SHA256 with server secret key (not reversible without key)
/// - Deterministic: same hostname + key always produces same cloak
/// - TLD preservation aids network debugging without exposing user identity
///
/// # Arguments
///
/// * `hostname` - Hostname to cloak (e.g., "user.example.com")
/// * `secret_key` - Server secret key for HMAC (MUST be kept private)
pub fn cloak_hostname(hostname: &str, secret_key: &str) -> String {
    // Step 1: HMAC-SHA256 the full hostname
    // SAFETY: HMAC-SHA256 accepts keys of any length per RFC 2104, this cannot fail
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(hostname.as_bytes());
    let result = mac.finalize();
    let hash_bytes = result.into_bytes();

    // Step 2: Base32 encode first 9 bytes for compact representation
    let hash_b32 = base32_encode(&hash_bytes[0..9]);

    // Step 3: Preserve TLD for readability (if present)
    if let Some(dot_pos) = hostname.rfind('.') {
        let tld_part = &hostname[dot_pos..];
        format!("{}{}", hash_b32, tld_part)
    } else {
        format!("{}.hidden", hash_b32)
    }
}

/// Check if a secret key is the insecure default.
///
/// Returns `true` if the key appears to be a placeholder that should be changed.
///
/// Checks for:
/// - Empty or too short secrets (< 16 chars)
/// - Known weak patterns ("changeme", "default", etc.)
/// - Low entropy (< 4 unique character classes)
pub fn is_default_secret(secret: &str) -> bool {
    // Basic length and pattern checks
    if secret.is_empty()
        || secret == "changeme"
        || secret.contains("default")
        || secret.contains("changeme")
        || secret.len() < 16
    {
        return true;
    }

    // Entropy check: good secrets should have character diversity
    // Check for presence of different character classes
    let has_lower = secret.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = secret.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = secret.chars().any(|c| c.is_ascii_digit());
    let has_special = secret.chars().any(|c| !c.is_alphanumeric());
    let unique_chars = secret.chars().collect::<std::collections::HashSet<_>>().len();

    // Require at least 3 character classes and 8 unique characters
    let char_classes = [has_lower, has_upper, has_digit, has_special]
        .iter()
        .filter(|&&b| b)
        .count();

    // Low entropy = likely a weak secret
    if char_classes < 3 || unique_chars < 8 {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    #[test]
    fn test_cloak_ipv4() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        let cloak = cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "ip");
        assert!(cloak.ends_with(".ip"));
        // Should be deterministic
        assert_eq!(cloak, cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "ip"));
        // Should have 4 segments
        assert_eq!(cloak.split('.').count(), 4);
    }

    #[test]
    fn test_cloak_ipv4_same_subnet() {
        // IPs in same /24 should produce same cloak (CIDR masking)
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.254".parse().unwrap();
        assert_eq!(
            cloak_ip_hmac_with_suffix(&ip1, TEST_SECRET, "ip"),
            cloak_ip_hmac_with_suffix(&ip2, TEST_SECRET, "ip")
        );
    }

    #[test]
    fn test_cloak_ipv4_different_subnet() {
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "192.168.2.1".parse().unwrap();
        assert_ne!(
            cloak_ip_hmac_with_suffix(&ip1, TEST_SECRET, "ip"),
            cloak_ip_hmac_with_suffix(&ip2, TEST_SECRET, "ip")
        );
    }

    #[test]
    fn test_cloak_ipv6() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let cloak = cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "ip");
        assert!(cloak.ends_with(":ip"));
        // Should be deterministic
        assert_eq!(cloak, cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "ip"));
    }

    #[test]
    fn test_cloak_hostname() {
        let hostname = "user.example.com";
        let cloak = cloak_hostname(hostname, TEST_SECRET);
        assert!(cloak.ends_with(".com"));
        // Should be deterministic
        assert_eq!(cloak, cloak_hostname(hostname, TEST_SECRET));

        // Different secret should produce different cloak
        let cloak2 = cloak_hostname(hostname, "different-secret");
        assert_ne!(cloak, cloak2);
        assert!(cloak2.ends_with(".com"));
    }

    #[test]
    fn test_cloak_hostname_no_tld() {
        let hostname = "localhost";
        let cloak = cloak_hostname(hostname, TEST_SECRET);
        assert!(cloak.ends_with(".hidden"));
    }

    #[test]
    fn test_custom_suffix() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let cloak = cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "straylight");
        assert!(cloak.ends_with(".straylight"));
    }

    #[test]
    fn test_is_default_secret() {
        assert!(is_default_secret(""));
        assert!(is_default_secret("changeme"));
        assert!(is_default_secret("default-salt"));
        assert!(is_default_secret("short")); // < 16 chars
        assert!(!is_default_secret("this-is-a-secure-production-key-2024"));
    }

    #[test]
    fn test_base32_encoding() {
        // Verify base32 output is lowercase alphanumeric only
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        let cloak = cloak_ip_hmac_with_suffix(&ip, TEST_SECRET, "ip");
        let hash_parts: Vec<&str> = cloak.split('.').collect();
        for part in &hash_parts[..3] {
            assert!(
                part.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            );
        }
    }
}
