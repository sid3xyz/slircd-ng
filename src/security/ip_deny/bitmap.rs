//! Bitmap helper functions for IPv4 address handling.

use std::net::Ipv4Addr;

/// Convert IPv4 address to u32 representation (big-endian).
#[inline]
pub(super) fn ipv4_to_u32(ip: &Ipv4Addr) -> u32 {
    u32::from_be_bytes(ip.octets())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_to_u32_localhost() {
        let ip = Ipv4Addr::new(127, 0, 0, 1);
        let result = ipv4_to_u32(&ip);
        // 127.0.0.1 = 127 * 2^24 + 0 * 2^16 + 0 * 2^8 + 1 = 2130706433
        assert_eq!(result, 0x7F000001);
        assert_eq!(result, 2130706433);
    }

    #[test]
    fn ipv4_to_u32_zeros() {
        let ip = Ipv4Addr::new(0, 0, 0, 0);
        assert_eq!(ipv4_to_u32(&ip), 0);
    }

    #[test]
    fn ipv4_to_u32_ones() {
        let ip = Ipv4Addr::new(255, 255, 255, 255);
        assert_eq!(ipv4_to_u32(&ip), 0xFFFFFFFF);
        assert_eq!(ipv4_to_u32(&ip), u32::MAX);
    }

    #[test]
    fn ipv4_to_u32_common_private() {
        // 192.168.1.1
        let ip = Ipv4Addr::new(192, 168, 1, 1);
        // 192 * 2^24 + 168 * 2^16 + 1 * 2^8 + 1
        let expected = (192 << 24) | (168 << 16) | (1 << 8) | 1;
        assert_eq!(ipv4_to_u32(&ip), expected);
        assert_eq!(ipv4_to_u32(&ip), 0xC0A80101);
    }

    #[test]
    fn ipv4_to_u32_class_a() {
        // 10.0.0.1
        let ip = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(ipv4_to_u32(&ip), 0x0A000001);
    }

    #[test]
    fn ipv4_to_u32_preserves_byte_order() {
        // Verify big-endian ordering
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let result = ipv4_to_u32(&ip);
        // First octet should be in the high byte position
        assert_eq!((result >> 24) & 0xFF, 1);
        assert_eq!((result >> 16) & 0xFF, 2);
        assert_eq!((result >> 8) & 0xFF, 3);
        assert_eq!(result & 0xFF, 4);
    }
}
