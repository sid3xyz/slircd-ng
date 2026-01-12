//! # slirc-proto
//!
//! A Rust library for parsing and serializing IRC protocol messages,
//! with full support for IRCv3 extensions.
//!
//! ## Features
//!
//! - IRC message parsing with tags, prefixes, commands, and parameters
//! - IRCv3 capability negotiation and message tags
//! - Zero-copy parsing with borrowed message types
//! - Optional Tokio integration for async networking
//! - User and channel mode parsing
//! - ISUPPORT (RPL_ISUPPORT) parsing
//! - Convenient message construction with builder pattern

#![deny(clippy::all)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! ## Quick Start
//!
//! ### Creating IRC Messages
//!
//! ```rust
//! use slirc_proto::{Message, prefix::Prefix};
//!
//! // Basic message construction
//! let privmsg = Message::privmsg("#rust", "Hello, world!");
//! let notice = Message::notice("nick", "Server notice");
//! let join = Message::join("#channel");
//!
//! // Messages with IRCv3 tags and prefixes
//! let tagged_msg = Message::privmsg("#dev", "Tagged message")
//!     .with_tag("time", Some("2023-01-01T12:00:00Z"))
//!     .with_tag("msgid", Some("abc123"))
//!     .with_prefix(Prefix::new_from_str("bot!bot@example.com"));
//!
//! println!("{}", tagged_msg); // Serializes to IRC protocol format
//! ```
//!
//! ### Parsing IRC Messages
//!
//! ```rust
//! use slirc_proto::Message;
//!
//! let raw = "@time=2023-01-01T12:00:00Z :nick!user@host PRIVMSG #channel :Hello!";
//! let message: Message = raw.parse().expect("Valid IRC message");
//!
//! if let Some(tags) = &message.tags {
//!     println!("Message has {} tags", tags.len());
//! }
//! ```
//!
//! ## Acknowledgments
//!
//! This project was inspired by the architectural patterns established by
//! [Aaron Weiss (aatxe)](https://github.com/aatxe) in the
//! [irc](https://github.com/aatxe/irc) crate. We are grateful for Aaron's
//! foundational work on IRC protocol handling in Rust.

pub mod caps;
pub mod chan;
pub mod colors;
pub mod command;
pub mod compliance;
pub mod crdt;
pub mod ctcp;
pub mod encode;
pub mod error;
pub mod format;
#[cfg(feature = "tokio")]
pub mod irc;
pub mod isupport;
#[cfg(feature = "tokio")]
pub mod line;
pub mod message;
pub mod mode;
pub mod nick;
pub mod prefix;
pub mod response;
pub mod sasl;
pub mod state;
pub mod util;

pub use self::caps::{Capability, NegotiationVersion};
pub use self::chan::ChannelExt;
pub use self::colors::FormattedStringExt;
pub use self::command::{
    BatchSubCommand, CapSubCommand, ChatHistorySubCommand, Command, MessageReference,
};
pub use self::compliance::{check_compliance, ComplianceConfig, ComplianceError};
pub use self::ctcp::{Ctcp, CtcpKind, CtcpOwned};
pub use self::encode::IrcEncode;
pub use self::nick::{NickExt, DEFAULT_NICK_MAX_LEN};

pub use self::command::CommandRef;
#[cfg(feature = "tokio")]
pub use self::irc::IrcCodec;
pub use self::isupport::{
    ChanModes, Isupport, IsupportBuilder, IsupportEntry, MaxList, PrefixSpec, TargMax,
};
pub use self::message::MessageRef;
pub use self::message::{Message, Tag};
pub use self::mode::{ChannelMode, Mode, UserMode};
pub use self::prefix::Prefix;
pub use self::prefix::PrefixRef;
pub use self::response::Response;
pub use self::sasl::{
    choose_mechanism, chunk_response, decode_base64, encode_external, encode_plain,
    encode_plain_with_authzid, needs_chunking, parse_mechanisms, SaslMechanism, SaslState,
    ScramClient, ScramError, ScramState, SASL_CHUNK_SIZE,
};
pub use self::state::{
    ConnectionState, HandshakeAction, HandshakeConfig, HandshakeError, HandshakeMachine,
    SaslCredentials,
};

pub mod casemap;
pub use self::casemap::{irc_eq, irc_lower_char, irc_to_lower};

pub use self::util::{matches_hostmask, wildcard_match};

pub mod ircv3;
pub use self::ircv3::{
    format_server_time, format_timestamp, generate_batch_ref, generate_msgid, parse_server_time,
};
pub mod scanner;
pub use scanner::{detect_protocol, is_non_irc_protocol, DetectedProtocol};

#[cfg(feature = "tokio")]
pub mod transport;
#[cfg(feature = "tokio")]
pub use self::transport::{
    LendingStream, Transport, TransportReadError, WebSocketNotSupportedError, ZeroCopyTransport,
    ZeroCopyTransportEnum, MAX_IRC_LINE_LEN,
};
#[cfg(feature = "tokio")]
pub use self::transport::{
    TransportParts, TransportRead, TransportReadHalf, TransportStream, TransportWrite,
    TransportWriteHalf,
};

#[cfg(feature = "tokio")]
pub mod websocket;
#[cfg(feature = "tokio")]
pub use self::websocket::{
    build_handshake_response, validate_handshake, HandshakeResult, WebSocketConfig,
};
