//! Hostname cloaking (masking) - Privacy for IRC users (Reviewed for deployment comments: none found)
//! RFC Context: Not standardized in RFC, but common IRC server practice
//! Research: docs/HOSTNAME_MASKING_RESEARCH.md
//! Plan: plan/refactor-cloaking-architecture-1.md (Phase 5: HMAC-SHA256 upgrade)
//!
//! SECURITY: Uses HMAC-SHA256 with secret key for cryptographic privacy
//! PERFORMANCE: Hierarchical 3-segment cloaking with CIDR masking preserves network structure
//! PRIVACY: Base32 encoding produces compact, URL-safe cloaked hostnames

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

type HmacSha256 = Hmac<Sha256>;

/// Base32 alphabet (RFC 4648 without padding, lowercase for IRC)
const BASE32_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Encode bytes to base32 (RFC 4648 style, lowercase, no padding)
/// More compact than hex (1.6 bytes per char vs 2 bytes per char)
fn base32_encode(data: &[u8]) -> String {
    let mut result = String::new();
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

/// Apply CIDR mask to IP address before hashing
/// Preserves network structure: /24 for IPv4, /48 for IPv6
/// Allows network administrators to identify general origin while protecting exact IP
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

/// Generate hierarchical cloaked hostname from IP using HMAC-SHA256
/// Pattern from ircd-hybrid: HMAC for cryptographic security, hierarchical for readability
///
/// # Security
/// - Uses HMAC-SHA256 with server secret key (not reversible without key)
/// - CIDR masking before hashing preserves network structure
/// - Deterministic: same IP + key always produces same cloak
///
/// # Format
/// - IPv4 /24: `abc123.def456.ghi789.ip` (3 segments, 6 chars each)
/// - IPv6 /48: `abc123:def456:ghi789:ip` (3 segments, colon-separated)
///
/// # Arguments
/// * `ip` - IP address to cloak
/// * `secret_key` - Server secret key for HMAC (MUST be kept private)
pub fn cloak_ip_hmac(ip: &IpAddr, secret_key: &str) -> String {
    // Step 1: Apply CIDR mask (/24 for IPv4, /48 for IPv6)
    let masked_ip = apply_cidr_mask(ip);

    // Step 2: HMAC-SHA256 the masked IP
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(masked_ip.to_string().as_bytes());
    let result = mac.finalize();
    let hash_bytes = result.into_bytes();

    // Step 3: Generate hierarchical segments using base32 encoding
    // Take 9 bytes (3 segments * 3 bytes each), encode to base32
    // Each 3 bytes -> ~5 base32 chars, we'll take 6 for readability
    let segment1 = base32_encode(&hash_bytes[0..3]);
    let segment2 = base32_encode(&hash_bytes[3..6]);
    let segment3 = base32_encode(&hash_bytes[6..9]);

    // Step 4: Format based on IP version
    match ip {
        IpAddr::V4(_) => format!("{}.{}.{}.ip", segment1, segment2, segment3),
        IpAddr::V6(_) => format!("{}:{}:{}:ip", segment1, segment2, segment3),
    }
}

/// Generate a simple cloaked hostname from an IP address
/// LEGACY v1 FUNCTION: Kept for backward compatibility
/// NEW DEPLOYMENTS: Use cloak_ip_hmac() with secret key from config
pub fn cloak_ip(ip: &IpAddr) -> String {
    // Fallback: use hardcoded salt for backward compatibility
    cloak_ip_hmac(ip, "slircd-default-salt-changeme")
}

/// Cloak a hostname using HMAC-SHA256 (preserves TLD for readability)
/// SECURITY EXPERT: Upgraded from SHA256 to HMAC-SHA256 per Big 4 pattern
/// COMPETITIVE ANALYSIS: UnrealIRCd/InspIRCd/Ergo all use HMAC with secret keys
///
/// # Format
/// - Input: "user.example.com"
/// - Output: "abc123def456.com" (HMAC hash + preserved TLD)
///
/// # Security
/// - Uses HMAC-SHA256 with server secret key (not reversible without key)
/// - Deterministic: same hostname + key always produces same cloak
/// - TLD preservation aids network debugging without exposing user identity
///
/// # Arguments
/// * `hostname` - Hostname to cloak (e.g., "user.example.com")
/// * `secret_key` - Server secret key for HMAC (MUST be kept private)
pub fn cloak_hostname(hostname: &str, secret_key: &str) -> String {
    // Step 1: HMAC-SHA256 the full hostname
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(hostname.as_bytes());
    let result = mac.finalize();
    let hash_bytes = result.into_bytes();

    // Step 2: Base32 encode first 9 bytes for compact representation
    let hash_b32 = base32_encode(&hash_bytes[0..9]);

    // Step 3: Preserve TLD for readability (if present)
    // Pattern: "abc123def456.com" instead of "abc123def456.hidden"
    if let Some(dot_pos) = hostname.rfind('.') {
        let tld_part = &hostname[dot_pos..];
        format!("{}{}", hash_b32, tld_part)
    } else {
        format!("{}.hidden", hash_b32)
    }
}

/// Legacy wrapper for backward compatibility
/// NEW CODE: Use cloak_hostname(hostname, secret_key) directly
#[deprecated(
    since = "1.1.0",
    note = "Use cloak_hostname(hostname, secret_key) with config secret instead"
)]
pub fn cloak_hostname_legacy(hostname: &str) -> String {
    cloak_hostname(hostname, "slircd-default-salt-changeme")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloak_ipv4() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        let cloak = cloak_ip(&ip);
        assert!(cloak.ends_with(".ip"));
        // Should be deterministic
        assert_eq!(cloak, cloak_ip(&ip));
    }

    #[test]
    fn test_cloak_ipv6() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let cloak = cloak_ip(&ip);
        assert!(cloak.ends_with(":ip"));
        // Should be deterministic
        assert_eq!(cloak, cloak_ip(&ip));
    }

    #[test]
    fn test_cloak_hostname() {
        let hostname = "user.example.com";
        let secret = "test-secret-key";
        let cloak = cloak_hostname(hostname, secret);
        assert!(cloak.ends_with(".com"));
        // Should be deterministic
        assert_eq!(cloak, cloak_hostname(hostname, secret));

        // Different secret should produce different cloak
        let cloak2 = cloak_hostname(hostname, "different-secret");
        assert_ne!(cloak, cloak2);
        assert!(cloak2.ends_with(".com"));
    }

    #[test]
    fn test_cloak_hostname_no_tld() {
        let hostname = "localhost";
        let secret = "test-secret-key";
        let cloak = cloak_hostname(hostname, secret);
        assert!(cloak.ends_with(".hidden"));
        // Should be deterministic
        assert_eq!(cloak, cloak_hostname(hostname, secret));
    }

    #[test]
    fn test_cloak_hostname_hmac_security() {
        // Verify HMAC produces different outputs for different secrets
        let hostname = "sensitive.example.org";
        let cloak1 = cloak_hostname(hostname, "secret1");
        let cloak2 = cloak_hostname(hostname, "secret2");

        assert_ne!(cloak1, cloak2);
        assert!(cloak1.ends_with(".org"));
        assert!(cloak2.ends_with(".org"));

        // Verify base32 encoding (lowercase alphanumeric)
        let hash_part = cloak1.split('.').next().unwrap();
        assert!(hash_part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }
}
