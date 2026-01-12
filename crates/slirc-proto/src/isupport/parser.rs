//! ISUPPORT parsing and data structures.

/// A single ISUPPORT key-value entry.
///
/// Represents a token from an ISUPPORT line, which can be either:
/// - A bare key (e.g., `EXCEPTS`) indicating a feature is supported
/// - A key=value pair (e.g., `NETWORK=Libera.Chat`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsupportEntry<'a> {
    /// The token key (e.g., `NETWORK`, `CHANTYPES`).
    pub key: &'a str,
    /// The optional value (e.g., `Libera.Chat` for `NETWORK=Libera.Chat`).
    pub value: Option<&'a str>,
}

/// Parsed ISUPPORT (005) server capabilities.
///
/// Contains all tokens from one or more `RPL_ISUPPORT` messages, providing
/// convenient accessors for common capabilities like `NETWORK`, `PREFIX`, etc.
///
/// # Example
///
/// ```
/// use slirc_proto::isupport::parse_params;
///
/// let tokens = ["NETWORK=TestNet", "CHANTYPES=#&", "PREFIX=(ov)@+"];
/// let isupport = parse_params(&tokens);
///
/// assert_eq!(isupport.network(), Some("TestNet"));
/// assert_eq!(isupport.chantypes(), Some("#&"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Isupport<'a> {
    entries: Vec<IsupportEntry<'a>>,
}

impl<'a> Isupport<'a> {
    /// Parse ISUPPORT from raw `RPL_ISUPPORT` response arguments.
    ///
    /// Skips the first argument (target nickname) and trailing text.
    pub fn from_response_args(args: &[&'a str]) -> Option<Self> {
        if args.is_empty() {
            return None;
        }

        let mut tokens = &args[1..];

        if let Some(last) = tokens.last() {
            if last.contains(' ') {
                tokens = &tokens[..tokens.len().saturating_sub(1)];
            }
        }
        Some(parse_params(tokens))
    }

    /// Parse ISUPPORT from an owned `Message`.
    ///
    /// Returns `None` if the message is not an `RPL_ISUPPORT` (005) response.
    pub fn from_message(msg: &'a crate::Message) -> Option<Self> {
        match &msg.command {
            crate::command::Command::Response(crate::response::Response::RPL_ISUPPORT, ref a) => {
                let borrowed: Vec<&'a str> = a.iter().map(|s| s.as_str()).collect();
                Self::from_response_args(&borrowed)
            }
            _ => None,
        }
    }

    /// Parse ISUPPORT from a borrowed `MessageRef`.
    ///
    /// Returns `None` if the message is not an `RPL_ISUPPORT` (005) response.
    pub fn from_message_ref(msg: &'a crate::MessageRef<'a>) -> Option<Self> {
        if let Ok(resp) = msg.command.name.parse::<crate::response::Response>() {
            if resp == crate::response::Response::RPL_ISUPPORT {
                let borrowed: Vec<&'a str> = msg.command.args.to_vec();
                return Self::from_response_args(&borrowed);
            }
        }
        None
    }

    /// Iterate over all parsed ISUPPORT entries.
    pub fn iter(&self) -> impl Iterator<Item = &IsupportEntry<'a>> {
        self.entries.iter()
    }

    /// Get the value for a specific ISUPPORT key.
    ///
    /// Returns `Some(Some(value))` if the key has a value,
    /// `Some(None)` if the key exists without a value,
    /// or `None` if the key is not present.
    pub fn get(&self, key: &str) -> Option<Option<&'a str>> {
        self.entries
            .iter()
            .rfind(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| e.value)
    }

    /// Get the `CASEMAPPING` value (e.g., `rfc1459`, `ascii`).
    pub fn casemapping(&self) -> Option<&'a str> {
        self.get("CASEMAPPING").flatten()
    }

    /// Get the `CHANTYPES` value (e.g., `#&`).
    pub fn chantypes(&self) -> Option<&'a str> {
        self.get("CHANTYPES").flatten()
    }

    /// Get the `NETWORK` name (e.g., `Libera.Chat`).
    pub fn network(&self) -> Option<&'a str> {
        self.get("NETWORK").flatten()
    }

    /// Parse the `PREFIX` token into a [`PrefixSpec`].
    pub fn prefix(&self) -> Option<PrefixSpec<'a>> {
        self.get("PREFIX").flatten().and_then(PrefixSpec::parse)
    }

    /// Parse the `CHANMODES` token into a [`ChanModes`] structure.
    pub fn chanmodes(&self) -> Option<ChanModes<'a>> {
        self.get("CHANMODES").flatten().and_then(ChanModes::parse)
    }

    /// Check if the server supports ban exceptions (`EXCEPTS`).
    pub fn has_excepts(&self) -> bool {
        self.get("EXCEPTS").is_some()
    }

    /// Get the mode character for ban exceptions (default: `e`).
    pub fn excepts_mode(&self) -> Option<char> {
        self.get("EXCEPTS").flatten().and_then(|s| s.chars().next())
    }

    /// Check if the server supports invite exceptions (`INVEX`).
    pub fn has_invex(&self) -> bool {
        self.get("INVEX").is_some()
    }

    /// Get the mode character for invite exceptions (default: `I`).
    pub fn invex_mode(&self) -> Option<char> {
        self.get("INVEX").flatten().and_then(|s| s.chars().next())
    }

    /// Parse the `TARGMAX` token into a [`TargMax`] structure.
    pub fn targmax(&self) -> Option<TargMax<'a>> {
        self.get("TARGMAX").flatten().and_then(TargMax::parse)
    }

    /// Parse the `MAXLIST` token into a [`MaxList`] structure.
    pub fn maxlist(&self) -> Option<MaxList> {
        self.get("MAXLIST").flatten().and_then(MaxList::parse)
    }
}

/// Parse ISUPPORT tokens from a slice of string parameters.
///
/// Tokens are parsed as `KEY` or `KEY=VALUE` pairs.
pub fn parse_params<'a>(params: &[&'a str]) -> Isupport<'a> {
    let mut entries = Vec::with_capacity(params.len());
    for &p in params {
        if p.starts_with(':') {
            break;
        }
        if p.is_empty() {
            continue;
        }
        let (k, v) = if let Some(eq) = p.find('=') {
            (&p[..eq], Some(&p[eq + 1..]))
        } else {
            (p, None)
        };

        entries.push(IsupportEntry { key: k, value: v });
    }
    Isupport { entries }
}

/// Parsed `PREFIX` ISUPPORT token.
///
/// Maps channel user modes (like `o`, `v`) to their prefix symbols (`@`, `+`).
///
/// # Example
///
/// ```
/// use slirc_proto::isupport::PrefixSpec;
///
/// let spec = PrefixSpec::parse("(ov)@+").unwrap();
/// assert_eq!(spec.prefix_for_mode('o'), Some('@'));
/// assert_eq!(spec.mode_for_prefix('+'), Some('v'));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixSpec<'a> {
    /// Mode characters (e.g., `ov` for operator and voice).
    pub modes: &'a str,
    /// Prefix symbols (e.g., `@+` for `@` and `+`).
    pub prefixes: &'a str,
}

impl<'a> PrefixSpec<'a> {
    /// Parse a `PREFIX` value like `(ov)@+`.
    pub fn parse(s: &'a str) -> Option<Self> {
        if let Some(open) = s.find('(') {
            if let Some(close) = s[open + 1..].find(')') {
                let close = open + 1 + close;
                let modes = &s[open + 1..close];
                let prefixes = &s[close + 1..];
                if !modes.is_empty() && !prefixes.is_empty() {
                    return Some(PrefixSpec { modes, prefixes });
                }
            }
        } else if !s.is_empty() {
            return Some(PrefixSpec {
                modes: "",
                prefixes: s,
            });
        }
        None
    }

    /// Returns true if the given character is a prefix mode on this server.
    ///
    /// This is useful for disambiguating modes like 'q' which can mean either
    /// Quiet (a list mode) or Founder/Owner (a prefix mode) depending on the
    /// IRC network. If `is_prefix_mode('q')` returns true, the server uses 'q'
    /// for founder/owner status.
    #[inline]
    pub fn is_prefix_mode(&self, mode: char) -> bool {
        self.modes.contains(mode)
    }

    /// Returns the prefix symbol for a given mode character.
    ///
    /// For example, with `PREFIX=(qaohv)~&@%+`:
    /// - `prefix_for_mode('o')` returns `Some('@')`
    /// - `prefix_for_mode('q')` returns `Some('~')`
    /// - `prefix_for_mode('x')` returns `None`
    #[inline]
    pub fn prefix_for_mode(&self, mode: char) -> Option<char> {
        self.modes
            .chars()
            .position(|c| c == mode)
            .and_then(|i| self.prefixes.chars().nth(i))
    }

    /// Returns the mode character for a given prefix symbol.
    ///
    /// For example, with `PREFIX=(qaohv)~&@%+`:
    /// - `mode_for_prefix('@')` returns `Some('o')`
    /// - `mode_for_prefix('~')` returns `Some('q')`
    /// - `mode_for_prefix('!')` returns `None`
    #[inline]
    pub fn mode_for_prefix(&self, prefix: char) -> Option<char> {
        self.prefixes
            .chars()
            .position(|c| c == prefix)
            .and_then(|i| self.modes.chars().nth(i))
    }
}

/// Parsed `CHANMODES` ISUPPORT token.
///
/// Channel modes are divided into four categories (A, B, C, D):
/// - **A**: List modes (e.g., `b` for ban)
/// - **B**: Modes with a parameter for both +/- (e.g., `k` for key)
/// - **C**: Modes with a parameter only for + (e.g., `l` for limit)
/// - **D**: Modes without parameters (e.g., `n` for no external messages)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChanModes<'a> {
    /// Type A: List modes (always have a parameter).
    pub a: &'a str,
    /// Type B: Modes that always require a parameter.
    pub b: &'a str,
    /// Type C: Modes that require a parameter when set.
    pub c: &'a str,
    /// Type D: Modes that never have a parameter.
    pub d: &'a str,
}

impl<'a> ChanModes<'a> {
    /// Parse a `CHANMODES` value like `b,k,l,imnpst`.
    pub fn parse(s: &'a str) -> Option<Self> {
        let mut parts = s.splitn(4, ',');
        let (a, b, c, d) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        Some(ChanModes { a, b, c, d })
    }
}

/// Parsed `TARGMAX` ISUPPORT token.
///
/// Specifies the maximum number of targets for various commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargMax<'a> {
    entries: Vec<(&'a str, Option<usize>)>,
}

impl<'a> TargMax<'a> {
    /// Parse a `TARGMAX` value like `PRIVMSG:4,NOTICE:4,JOIN:`.
    pub fn parse(s: &'a str) -> Option<Self> {
        if s.is_empty() {
            return Some(TargMax {
                entries: Vec::new(),
            });
        }
        let mut entries = Vec::new();
        for part in s.split(',') {
            if part.is_empty() {
                continue;
            }
            if let Some(colon) = part.find(':') {
                let (cmd, num) = (&part[..colon], &part[colon + 1..]);
                let val = num.parse::<usize>().ok();
                if !cmd.is_empty() {
                    entries.push((cmd, val));
                }
            } else {
                entries.push((part, None));
            }
        }
        Some(TargMax { entries })
    }

    /// Get the target limit for a specific command.
    pub fn get(&self, cmd: &str) -> Option<Option<usize>> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(cmd))
            .map(|(_, v)| *v)
    }

    /// Iterate over all command/limit pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, Option<usize>)> + '_ {
        self.entries.iter().copied()
    }
}

/// Parsed `MAXLIST` ISUPPORT token.
///
/// Specifies the maximum number of entries for list modes (bans, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxList {
    entries: Vec<(char, usize)>,
}

impl MaxList {
    /// Parse a `MAXLIST` value like `b:100,e:100,I:100`.
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() {
            return Some(MaxList {
                entries: Vec::new(),
            });
        }
        let mut entries: Vec<(char, usize)> = Vec::new();
        for part in s.split(',') {
            if part.is_empty() {
                continue;
            }
            let (modes, limit_str) = part.split_once(':')?;

            let limit: usize = match limit_str.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };
            for ch in modes.chars() {
                entries.retain(|(c, _)| *c != ch);
                entries.push((ch, limit));
            }
        }
        Some(MaxList { entries })
    }

    /// Get the limit for a specific list mode character.
    pub fn limit_for(&self, mode: char) -> Option<usize> {
        self.entries
            .iter()
            .rev()
            .find(|(c, _)| *c == mode)
            .map(|(_, n)| *n)
    }

    /// Iterate over all mode/limit pairs.
    pub fn iter(&self) -> impl Iterator<Item = (char, usize)> + '_ {
        self.entries.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_spec_is_prefix_mode() {
        let spec = PrefixSpec::parse("(qaohv)~&@%+").unwrap();

        // These are prefix modes
        assert!(spec.is_prefix_mode('q')); // founder/owner
        assert!(spec.is_prefix_mode('a')); // admin
        assert!(spec.is_prefix_mode('o')); // operator
        assert!(spec.is_prefix_mode('h')); // halfop
        assert!(spec.is_prefix_mode('v')); // voice

        // These are not prefix modes
        assert!(!spec.is_prefix_mode('b')); // ban
        assert!(!spec.is_prefix_mode('i')); // invite-only
        assert!(!spec.is_prefix_mode('x')); // nonexistent
    }

    #[test]
    fn prefix_spec_prefix_for_mode() {
        let spec = PrefixSpec::parse("(qaohv)~&@%+").unwrap();

        assert_eq!(spec.prefix_for_mode('q'), Some('~'));
        assert_eq!(spec.prefix_for_mode('a'), Some('&'));
        assert_eq!(spec.prefix_for_mode('o'), Some('@'));
        assert_eq!(spec.prefix_for_mode('h'), Some('%'));
        assert_eq!(spec.prefix_for_mode('v'), Some('+'));
        assert_eq!(spec.prefix_for_mode('x'), None);
    }

    #[test]
    fn prefix_spec_mode_for_prefix() {
        let spec = PrefixSpec::parse("(qaohv)~&@%+").unwrap();

        assert_eq!(spec.mode_for_prefix('~'), Some('q'));
        assert_eq!(spec.mode_for_prefix('&'), Some('a'));
        assert_eq!(spec.mode_for_prefix('@'), Some('o'));
        assert_eq!(spec.mode_for_prefix('%'), Some('h'));
        assert_eq!(spec.mode_for_prefix('+'), Some('v'));
        assert_eq!(spec.mode_for_prefix('!'), None);
    }

    #[test]
    fn prefix_spec_standard_ov_only() {
        // Minimal PREFIX=(ov)@+ as seen on many servers
        let spec = PrefixSpec::parse("(ov)@+").unwrap();

        assert!(spec.is_prefix_mode('o'));
        assert!(spec.is_prefix_mode('v'));
        assert!(!spec.is_prefix_mode('q')); // q would be quiet, not founder

        assert_eq!(spec.prefix_for_mode('o'), Some('@'));
        assert_eq!(spec.prefix_for_mode('v'), Some('+'));
    }
}
