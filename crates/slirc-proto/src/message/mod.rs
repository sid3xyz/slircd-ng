//! IRC message types and parsing.

mod borrowed;
mod nom_parser;
mod parse;
mod serialize;
/// IRCv3 tag utilities.
pub mod tags;
mod types;

pub use self::borrowed::MessageRef;
pub use self::types::{Message, Tag};
