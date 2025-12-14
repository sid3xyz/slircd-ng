//! Bitmap helper functions for IPv4 address handling.

use std::net::Ipv4Addr;

/// Convert IPv4 address to u32 representation (big-endian).
#[inline]
pub(super) fn ipv4_to_u32(ip: &Ipv4Addr) -> u32 {
    u32::from_be_bytes(ip.octets())
}
