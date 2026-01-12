//! Zero-copy transport for high-performance message parsing.
//!
//! This module provides zero-copy transports that parse IRC messages
//! directly from internal buffers, yielding borrowed [`MessageRef`] values
//! without heap allocations.
//!
//! # Core Types
//!
//! - [`LendingStream`]: GAT-based trait for zero-copy iteration
//! - [`ZeroCopyTransport`]: TCP/TLS zero-copy transport
//! - [`ZeroCopyWebSocketTransport`]: WebSocket zero-copy transport
//! - [`ZeroCopyTransportEnum`]: Unified enum wrapper for all transport types
//!
//! # Performance
//!
//! These transports are designed for hot loops where allocations are expensive:
//! - No heap allocations per message
//! - Minimal buffer management overhead
//! - Direct parsing from byte buffer
//!
//! # Example
//!
//! ```ignore
//! let mut transport = ZeroCopyTransport::new(tcp_stream);
//! while let Some(result) = transport.next().await {
//!     let msg_ref = result?;
//!     // Process msg_ref - it borrows from transport's buffer
//! }
//! ```

pub mod helpers;
pub mod tcp;
pub mod trait_def;
pub mod unified;

#[cfg(feature = "tokio")]
pub mod websocket;

// Re-export all public types
pub use self::trait_def::LendingStream;
pub use tcp::ZeroCopyTransport;
pub use unified::ZeroCopyTransportEnum;

#[cfg(feature = "tokio")]
pub use websocket::ZeroCopyWebSocketTransport;
