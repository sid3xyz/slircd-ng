//! PROXY protocol (v1/v2) parser.
//!
//! Supports parsing PROXY protocol headers to extract the real client IP
//! when running behind a load balancer (e.g., HAProxy, AWS ELB).

use anyhow::{Result, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

/// Max length of a PROXY protocol header.
/// v1: 107 bytes
/// v2: header (16) + max addr (216) = 232 bytes
const MAX_HEADER_LEN: usize = 256;

/// Parse PROXY protocol header from the stream.
///
/// This function reads from the stream to parse the header.
/// It consumes the header bytes.
pub async fn parse_proxy_header(stream: &mut TcpStream) -> Result<SocketAddr> {
    // Peek first few bytes to determine version
    let mut buf = [0u8; 16];
    let n = stream.peek(&mut buf).await?;
    if n < 12 {
        // Wait for more data or fail?
        // For now, let's try to read strictly.
        // Actually, peek might return less than requested.
        // We should probably just try to read the signature.
    }

    // Check for v2 signature
    // \x0D\x0A\x0D\x0A\x00\x0D\x0A\x51\x55\x49\x54\x0A
    let v2_sig = [
        0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
    ];

    if n >= 12 && buf[..12] == v2_sig {
        return parse_v2(stream).await;
    }

    // Check for v1 signature "PROXY "
    if n >= 6 && &buf[..6] == b"PROXY " {
        return parse_v1(stream).await;
    }

    bail!("Invalid PROXY protocol header signature");
}

async fn parse_v1(stream: &mut TcpStream) -> Result<SocketAddr> {
    // v1 is text-based, terminated by \r\n.
    // We need to read byte by byte until \r\n, but we can't buffer too much
    // because we can't put back bytes into TcpStream for the next handler.
    // However, since we are passing the TcpStream to TlsAcceptor or Connection,
    // and they expect a raw stream, we MUST NOT read more than the header.

    // Since TcpStream doesn't support "unread", reading byte-by-byte is safe but slow.
    // But PROXY header is short.

    let mut line = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        stream.read_exact(&mut byte).await?;
        line.push(byte[0]);

        if line.len() > MAX_HEADER_LEN {
            bail!("PROXY v1 header too long");
        }

        if line.ends_with(b"\r\n") {
            break;
        }
    }

    let header = String::from_utf8(line)?;
    let parts: Vec<&str> = header.trim().split(' ').collect();

    // PROXY TCP4 255.255.255.255 255.255.255.255 65535 65535
    if parts.len() < 6 {
        bail!("Invalid PROXY v1 header format");
    }

    if parts[0] != "PROXY" {
        bail!("Invalid PROXY v1 header signature");
    }

    let _proto = parts[1]; // TCP4, TCP6, UNKNOWN
    let src_ip = parts[2];
    let _dst_ip = parts[3];
    let src_port = parts[4];
    let _dst_port = parts[5];

    let ip: IpAddr = src_ip.parse()?;
    let port: u16 = src_port.parse()?;

    Ok(SocketAddr::new(ip, port))
}

async fn parse_v2(stream: &mut TcpStream) -> Result<SocketAddr> {
    // Read signature (12 bytes)
    let mut sig = [0u8; 12];
    stream.read_exact(&mut sig).await?;

    // Read version/command (1 byte)
    let mut ver_cmd = [0u8; 1];
    stream.read_exact(&mut ver_cmd).await?;

    let ver = (ver_cmd[0] & 0xF0) >> 4;
    let cmd = ver_cmd[0] & 0x0F;

    if ver != 2 {
        bail!("Unsupported PROXY protocol version: {}", ver);
    }

    if cmd == 0 {
        // LOCAL command, ignore address info, keep local connection info
        // But we need to skip the rest of the header.
        // Read family/transport (1 byte)
        let mut fam_trans = [0u8; 1];
        stream.read_exact(&mut fam_trans).await?;

        // Read length (2 bytes)
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await?;
        let len = u16::from_be_bytes(len_buf);

        // Skip len bytes
        let mut skip = vec![0u8; len as usize];
        stream.read_exact(&mut skip).await?;

        // Return error to signal "use local address"?
        // Or we should return the local address of the socket?
        // The caller has the original socket address.
        // If we return error, the connection is rejected.
        // We should probably return a special error or handle this.
        // For now, let's bail, as we expect PROXY to provide real IP.
        bail!("PROXY LOCAL command not supported yet");
    }

    if cmd != 1 {
        bail!("Unsupported PROXY command: {}", cmd);
    }

    // Read family/transport (1 byte)
    let mut fam_trans = [0u8; 1];
    stream.read_exact(&mut fam_trans).await?;

    let family = (fam_trans[0] & 0xF0) >> 4;
    let _transport = fam_trans[0] & 0x0F;

    // Read length (2 bytes)
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf);

    // Read address data
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).await?;

    match family {
        1 => {
            // AF_INET (IPv4)
            if data.len() < 12 {
                bail!("Invalid IPv4 data length");
            }
            let src_ip = Ipv4Addr::new(data[0], data[1], data[2], data[3]);
            // dst_ip at 4..8
            let src_port = u16::from_be_bytes([data[8], data[9]]);
            // dst_port at 10..12
            Ok(SocketAddr::new(IpAddr::V4(src_ip), src_port))
        }
        2 => {
            // AF_INET6 (IPv6)
            if data.len() < 36 {
                bail!("Invalid IPv6 data length");
            }
            let mut ip_bytes = [0u8; 16];
            ip_bytes.copy_from_slice(&data[0..16]);
            let src_ip = Ipv6Addr::from(ip_bytes);
            // dst_ip at 16..32
            let src_port = u16::from_be_bytes([data[32], data[33]]);
            // dst_port at 34..36
            Ok(SocketAddr::new(IpAddr::V6(src_ip), src_port))
        }
        _ => bail!("Unsupported address family: {}", family),
    }
}
