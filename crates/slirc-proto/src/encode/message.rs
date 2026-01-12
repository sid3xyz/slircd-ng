//! Encoding implementations for IRC messages and prefixes.

use std::io::{self, Write};

use crate::message::tags::escape_tag_value_to_writer;
use crate::message::{Message, MessageRef, Tag};
use crate::prefix::Prefix;

use super::IrcEncode;

/// Encode a single tag to the writer.
fn encode_tag<W: Write>(w: &mut W, tag: &Tag) -> io::Result<usize> {
    let mut written = w.write(tag.0.as_bytes())?;
    if let Some(ref value) = tag.1 {
        written += w.write(b"=")?;
        written += escape_tag_value_to_writer(w, value)?;
    }
    Ok(written)
}

impl IrcEncode for Message {
    fn encode<W: Write>(&self, w: &mut W) -> io::Result<usize> {
        let mut written = 0;

        // Tags
        if let Some(ref tags) = self.tags {
            written += w.write(b"@")?;
            for (i, tag) in tags.iter().enumerate() {
                if i > 0 {
                    written += w.write(b";")?;
                }
                written += encode_tag(w, tag)?;
            }
            written += w.write(b" ")?;
        }

        // Prefix
        if let Some(ref prefix) = self.prefix {
            written += w.write(b":")?;
            written += prefix.encode(w)?;
            written += w.write(b" ")?;
        }

        // Command
        written += self.command.encode(w)?;

        // CRLF
        written += w.write(b"\r\n")?;

        Ok(written)
    }
}

impl<'a> IrcEncode for MessageRef<'a> {
    fn encode<W: Write>(&self, w: &mut W) -> io::Result<usize> {
        let mut written = 0;

        // Tags (raw, already formatted)
        if let Some(tags) = self.tags {
            written += w.write(b"@")?;
            written += w.write(tags.as_bytes())?;
            written += w.write(b" ")?;
        }

        // Prefix
        if let Some(ref prefix) = self.prefix {
            written += w.write(b":")?;
            written += w.write(prefix.raw.as_bytes())?;
            written += w.write(b" ")?;
        }

        // Command (raw)
        written += w.write(self.command.name.as_bytes())?;
        for arg in &self.command.args {
            written += w.write(b" ")?;
            written += w.write(arg.as_bytes())?;
        }

        // CRLF
        written += w.write(b"\r\n")?;

        Ok(written)
    }
}

impl IrcEncode for Prefix {
    fn encode<W: Write>(&self, w: &mut W) -> io::Result<usize> {
        match self {
            Prefix::ServerName(name) => w.write(name.as_bytes()),
            Prefix::Nickname(nick, user, host) => {
                let mut written = w.write(nick.as_bytes())?;
                if !user.is_empty() {
                    written += w.write(b"!")?;
                    written += w.write(user.as_bytes())?;
                }
                if !host.is_empty() {
                    written += w.write(b"@")?;
                    written += w.write(host.as_bytes())?;
                }
                Ok(written)
            }
        }
    }
}
