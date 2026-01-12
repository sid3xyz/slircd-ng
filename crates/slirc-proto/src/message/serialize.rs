use std::fmt::{self, Display, Formatter};

use super::tags::escape_tag_value;
use super::types::Message;

impl Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(ref tags) = self.tags {
            write!(f, "@")?;

            for (i, tag) in tags.iter().enumerate() {
                if i > 0 {
                    write!(f, ";")?;
                }

                write!(f, "{}", tag.0)?;

                if let Some(ref value) = tag.1 {
                    write!(f, "=")?;
                    escape_tag_value(f, value)?;
                }
            }

            write!(f, " ")?;
        }

        if let Some(ref prefix) = self.prefix {
            write!(f, ":{} ", prefix)?;
        }

        write!(f, "{}\r\n", self.command)
    }
}
