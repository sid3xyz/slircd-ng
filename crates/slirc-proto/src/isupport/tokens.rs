//! ISUPPORT builder for server responses.

/// Builder for constructing ISUPPORT token strings.
///
/// Useful for IRC servers to generate `RPL_ISUPPORT` responses.
#[derive(Debug, Clone, Default)]
pub struct IsupportBuilder {
    tokens: Vec<String>,
}

impl IsupportBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    /// Set the `NETWORK` token.
    pub fn network(mut self, name: &str) -> Self {
        self.tokens.push(format!("NETWORK={}", name));
        self
    }

    /// Set the `CHANTYPES` token.
    pub fn chantypes(mut self, types: &str) -> Self {
        self.tokens.push(format!("CHANTYPES={}", types));
        self
    }

    /// Set the `CHANMODES` token.
    pub fn chanmodes(mut self, modes: &str) -> Self {
        self.tokens.push(format!("CHANMODES={}", modes));
        self
    }

    /// Set the `PREFIX` token with symbols and mode letters.
    pub fn prefix(mut self, symbols: &str, letters: &str) -> Self {
        self.tokens.push(format!("PREFIX=({}){}", letters, symbols));
        self
    }

    /// Set the `CASEMAPPING` token.
    pub fn casemapping(mut self, mapping: &str) -> Self {
        self.tokens.push(format!("CASEMAPPING={}", mapping));
        self
    }

    /// Set the `MAXCHANNELS` token.
    pub fn max_channels(mut self, count: u32) -> Self {
        self.tokens.push(format!("MAXCHANNELS={}", count));
        self
    }

    /// Set the `NICKLEN` token.
    pub fn max_nick_length(mut self, len: u32) -> Self {
        self.tokens.push(format!("NICKLEN={}", len));
        self
    }

    /// Set the `TOPICLEN` token.
    pub fn max_topic_length(mut self, len: u32) -> Self {
        self.tokens.push(format!("TOPICLEN={}", len));
        self
    }

    /// Set the `MODES` token (max modes per command).
    pub fn modes_count(mut self, count: u32) -> Self {
        self.tokens.push(format!("MODES={}", count));
        self
    }

    /// Set the `STATUSMSG` token.
    pub fn status_msg(mut self, symbols: &str) -> Self {
        self.tokens.push(format!("STATUSMSG={}", symbols));
        self
    }

    /// Set the `EXCEPTS` token.
    pub fn excepts(mut self, mode_char: Option<char>) -> Self {
        if let Some(c) = mode_char {
            self.tokens.push(format!("EXCEPTS={}", c));
        } else {
            self.tokens.push("EXCEPTS".to_string());
        }
        self
    }

    /// Set the `INVEX` token.
    pub fn invex(mut self, mode_char: Option<char>) -> Self {
        if let Some(c) = mode_char {
            self.tokens.push(format!("INVEX={}", c));
        } else {
            self.tokens.push("INVEX".to_string());
        }
        self
    }

    /// Add a custom token.
    pub fn custom(mut self, key: &str, value: Option<&str>) -> Self {
        if let Some(v) = value {
            self.tokens.push(format!("{}={}", key, v));
        } else {
            self.tokens.push(key.to_string());
        }
        self
    }

    /// Build the tokens into a single space-separated string.
    pub fn build(self) -> String {
        self.tokens.join(" ")
    }

    /// Build the tokens into multiple lines, each with at most `max_per_line` tokens.
    pub fn build_lines(self, max_per_line: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = Vec::new();

        for token in self.tokens {
            current.push(token);
            if current.len() >= max_per_line {
                lines.push(current.join(" "));
                current.clear();
            }
        }

        if !current.is_empty() {
            lines.push(current.join(" "));
        }

        lines
    }

    /// Set the `TARGMAX` token using a builder.
    pub fn targmax(mut self, builder: TargMaxBuilder) -> Self {
        self.tokens.push(format!("TARGMAX={}", builder.build()));
        self
    }

    /// Set the `CHANMODES` token using a builder.
    pub fn chanmodes_typed(mut self, builder: ChanModesBuilder) -> Self {
        self.tokens.push(format!("CHANMODES={}", builder.build()));
        self
    }
}

/// Builder for `TARGMAX` ISUPPORT token.
///
/// Specifies the maximum number of targets for various commands.
/// - `CMD:limit` means `CMD` accepts at most `limit` targets.
/// - `CMD:` means `CMD` accepts unlimited targets.
#[derive(Debug, Clone, Default)]
pub struct TargMaxBuilder {
    entries: Vec<(String, Option<usize>)>,
}

impl TargMaxBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command with a specific limit.
    pub fn add(mut self, cmd: &str, limit: usize) -> Self {
        self.entries.push((cmd.to_uppercase(), Some(limit)));
        self
    }

    /// Add a command with unlimited targets.
    pub fn add_unlimited(mut self, cmd: &str) -> Self {
        self.entries.push((cmd.to_uppercase(), None));
        self
    }

    /// Build the TARGMAX string.
    pub fn build(&self) -> String {
        self.entries
            .iter()
            .map(|(cmd, limit)| match limit {
                Some(l) => format!("{}:{}", cmd, l),
                None => format!("{}:", cmd),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Builder for `CHANMODES` ISUPPORT token.
///
/// Ensures disjoint sets for the four mode categories (A, B, C, D).
/// - Type A: List modes (e.g., `b`, `e`, `I`)
/// - Type B: Parameter always (e.g., `k`)
/// - Type C: Parameter when set (e.g., `l`)
/// - Type D: No parameter (e.g., `i`, `m`, `n`, `s`, `t`)
#[derive(Debug, Clone, Default)]
pub struct ChanModesBuilder {
    a: String,
    b: String,
    c: String,
    d: String,
}

impl ChanModesBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set Type A modes (List modes).
    pub fn list_modes(mut self, modes: &str) -> Self {
        self.validate_unique(modes);
        self.a = modes.to_string();
        self
    }

    /// Set Type B modes (Parameter always).
    pub fn param_always(mut self, modes: &str) -> Self {
        self.validate_unique(modes);
        self.b = modes.to_string();
        self
    }

    /// Set Type C modes (Parameter when set).
    pub fn param_set(mut self, modes: &str) -> Self {
        self.validate_unique(modes);
        self.c = modes.to_string();
        self
    }

    /// Set Type D modes (No parameter).
    pub fn no_param(mut self, modes: &str) -> Self {
        self.validate_unique(modes);
        self.d = modes.to_string();
        self
    }

    /// Validate that the new modes don't conflict with existing ones or contain duplicates.
    fn validate_unique(&self, new_modes: &str) {
        let mut seen_in_new = std::collections::HashSet::new();
        for char in new_modes.chars() {
            if !seen_in_new.insert(char) {
                panic!(
                    "Duplicate channel mode character '{}' found in input string.",
                    char
                );
            }
            if self.a.contains(char)
                || self.b.contains(char)
                || self.c.contains(char)
                || self.d.contains(char)
            {
                panic!("Duplicate channel mode character '{}' found in CHANMODES. Modes must be disjoint.", char);
            }
        }
    }

    /// Build the CHANMODES string.
    pub fn build(&self) -> String {
        format!("{},{},{},{}", self.a, self.b, self.c, self.d)
    }
}
