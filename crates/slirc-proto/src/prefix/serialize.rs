use std::fmt;

use super::types::Prefix;

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Prefix::ServerName(name) => write!(f, "{}", name),
            Prefix::Nickname(name, user, host) => match (&name[..], &user[..], &host[..]) {
                ("", "", "") => write!(f, ""),
                (name, "", "") => write!(f, "{}", name),
                (name, user, "") => write!(f, "{}!{}", name, user),
                (name, "", host) => write!(f, "{}@{}", name, host),
                (name, user, host) => write!(f, "{}!{}@{}", name, user, host),
            },
        }
    }
}
