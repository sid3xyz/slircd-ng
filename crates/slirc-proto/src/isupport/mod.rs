//! ISUPPORT (RPL_ISUPPORT / 005) parsing for IRC servers.
//!
//! This module provides types for parsing and querying the server capability
//! tokens sent in `RPL_ISUPPORT` (numeric 005) replies.
//!
//! # Reference
//! - Modern IRC documentation: <https://modern.ircdocs.horse/isupport.html>

mod parser;
mod tokens;

pub use parser::{parse_params, ChanModes, Isupport, IsupportEntry, MaxList, PrefixSpec, TargMax};
pub use tokens::{ChanModesBuilder, IsupportBuilder, TargMaxBuilder};
