use std::borrow::Cow;

use crate::chan::ChannelExt;
use crate::command::Command;
use crate::error;
use crate::error::MessageParseError;
use crate::prefix::Prefix;

/// An owned IRC message.
///
/// Contains the complete parsed representation of an IRC message including
/// optional IRCv3 tags, optional prefix/source, and the command with parameters.
///
/// # Example
///
/// ```
/// use slirc_proto::Message;
///
/// // Parse a message
/// let msg: Message = ":nick!user@host PRIVMSG #channel :Hello!".parse().unwrap();
///
/// // Construct a message
/// let msg = Message::privmsg("#channel", "Hello!");
/// ```
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message {
    /// IRCv3 message tags (e.g., `time`, `msgid`).
    pub tags: Option<Vec<Tag>>,
    /// Message prefix/source (e.g., `nick!user@host`).
    pub prefix: Option<Prefix>,
    /// The IRC command and its parameters.
    pub command: Command,
}

impl Message {
    /// Create a new message from raw components.
    #[must_use = "message creation result should be handled"]
    pub fn new(
        prefix: Option<&str>,
        command: &str,
        args: Vec<&str>,
    ) -> Result<Message, MessageParseError> {
        Message::with_tags(None, prefix, command, args)
    }

    /// Create a new message with tags from raw components.
    #[must_use = "message creation result should be handled"]
    pub fn with_tags(
        tags: Option<Vec<Tag>>,
        prefix: Option<&str>,
        command: &str,
        args: Vec<&str>,
    ) -> Result<Message, error::MessageParseError> {
        let parsed_prefix = if let Some(p) = prefix {
            Some(crate::prefix::Prefix::try_from_str(p)?)
        } else {
            None
        };

        Ok(Message {
            tags,
            prefix: parsed_prefix,
            command: Command::new(command, args)?,
        })
    }

    /// Get the nickname from the message prefix, if present.
    pub fn source_nickname(&self) -> Option<&str> {
        self.prefix.as_ref().and_then(|p| match p {
            Prefix::Nickname(name, _, _) => Some(&name[..]),
            _ => None,
        })
    }

    /// Get the appropriate target for a response.
    ///
    /// For channel messages, returns the channel name.
    /// For private messages, returns the sender's nickname.
    pub fn response_target(&self) -> Option<&str> {
        match self.command {
            Command::PRIVMSG(ref target, _) if target.is_channel_name() => Some(target),
            Command::NOTICE(ref target, _) if target.is_channel_name() => Some(target),
            _ => self.source_nickname(),
        }
    }

    /// Get the value of an IRCv3 tag by key.
    pub fn tag_value(&self, key: &str) -> Option<&str> {
        self.tags
            .as_ref()?
            .iter()
            .find(|Tag(k, _)| k.as_ref() == key)
            .and_then(|Tag(_, v)| v.as_deref())
    }

    /// Get the server-time tag value.
    pub fn server_time(&self) -> Option<&str> {
        self.tag_value("time")
    }

    /// Get the labeled-response label tag.
    pub fn label(&self) -> Option<&str> {
        self.tag_value("label")
    }

    /// Get the message ID tag.
    pub fn msgid(&self) -> Option<&str> {
        self.tag_value("msgid")
    }

    /// Get the account tag value.
    pub fn account_tag(&self) -> Option<&str> {
        self.tag_value("account")
    }
    /// Create a PRIVMSG message to a target with text
    #[must_use]
    pub fn privmsg<T, M>(target: T, text: M) -> Self
    where
        T: Into<String>,
        M: Into<String>,
    {
        Command::PRIVMSG(target.into(), text.into()).into()
    }

    /// Create a NOTICE message to a target with text
    #[must_use]
    pub fn notice<T, M>(target: T, text: M) -> Self
    where
        T: Into<String>,
        M: Into<String>,
    {
        Command::NOTICE(target.into(), text.into()).into()
    }

    /// Create a JOIN message for a channel
    #[must_use]
    pub fn join<C>(channel: C) -> Self
    where
        C: Into<String>,
    {
        Command::JOIN(channel.into(), None, None).into()
    }

    /// Create a JOIN message for a channel with a key
    #[must_use]
    pub fn join_with_key<C, K>(channel: C, key: K) -> Self
    where
        C: Into<String>,
        K: Into<String>,
    {
        Command::JOIN(channel.into(), Some(key.into()), None).into()
    }

    /// Create a PART message to leave a channel
    #[must_use]
    pub fn part<C>(channel: C) -> Self
    where
        C: Into<String>,
    {
        Command::PART(channel.into(), None).into()
    }

    /// Create a PART message to leave a channel with a message
    #[must_use]
    pub fn part_with_message<C, M>(channel: C, message: M) -> Self
    where
        C: Into<String>,
        M: Into<String>,
    {
        Command::PART(channel.into(), Some(message.into())).into()
    }

    /// Create a NICK message to change nickname
    #[must_use]
    pub fn nick<N>(nickname: N) -> Self
    where
        N: Into<String>,
    {
        Command::NICK(nickname.into()).into()
    }

    /// Create a USER message for registration
    #[must_use]
    pub fn user<U, R>(username: U, realname: R) -> Self
    where
        U: Into<String>,
        R: Into<String>,
    {
        Command::USER(username.into(), "0".into(), realname.into()).into()
    }

    /// Create a PING message to a server
    #[must_use]
    pub fn ping<S>(server: S) -> Self
    where
        S: Into<String>,
    {
        Command::PING(server.into(), None).into()
    }

    /// Create a PONG message in response to a PING
    #[must_use]
    pub fn pong<S>(server: S) -> Self
    where
        S: Into<String>,
    {
        Command::PONG(server.into(), None).into()
    }

    /// Create a PONG message with server name and token
    ///
    /// The proper format is `PONG <server> <token>` where server is the
    /// name of the responding server and token is the value from the PING.
    #[must_use]
    pub fn pong_with_token<S, T>(server: S, token: T) -> Self
    where
        S: Into<String>,
        T: Into<String>,
    {
        Command::PONG(server.into(), Some(token.into())).into()
    }

    /// Create a QUIT message
    #[must_use]
    pub fn quit() -> Self {
        Command::QUIT(None).into()
    }

    /// Create a QUIT message with a quit message
    #[must_use]
    pub fn quit_with_message<M>(message: M) -> Self
    where
        M: Into<String>,
    {
        Command::QUIT(Some(message.into())).into()
    }

    /// Create a KICK message
    #[must_use]
    pub fn kick<C, N>(channel: C, nickname: N) -> Self
    where
        C: Into<String>,
        N: Into<String>,
    {
        Command::KICK(channel.into(), nickname.into(), None).into()
    }

    /// Create a KICK message with a reason
    #[must_use]
    pub fn kick_with_reason<C, N, R>(channel: C, nickname: N, reason: R) -> Self
    where
        C: Into<String>,
        N: Into<String>,
        R: Into<String>,
    {
        Command::KICK(channel.into(), nickname.into(), Some(reason.into())).into()
    }

    /// Create an AWAY message
    #[must_use]
    pub fn away() -> Self {
        Command::AWAY(None).into()
    }

    /// Create an AWAY message with a message
    #[must_use]
    pub fn away_with_message<M>(message: M) -> Self
    where
        M: Into<String>,
    {
        Command::AWAY(Some(message.into())).into()
    }

    /// Add IRCv3 tags to this message
    #[must_use]
    pub fn with_message_tags(mut self, tags: Vec<Tag>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Add a single IRCv3 tag to this message
    #[must_use]
    pub fn with_tag<K, V>(mut self, key: K, value: Option<V>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        let tag = Tag::new(key, value.map(|v| v.into()));
        if let Some(ref mut existing_tags) = self.tags {
            existing_tags.push(tag);
        } else {
            self.tags = Some(vec![tag]);
        }
        self
    }

    /// Set the prefix/source of this message
    #[must_use]
    pub fn with_prefix(mut self, prefix: crate::prefix::Prefix) -> Self {
        self.prefix = Some(prefix);
        self
    }
}

impl From<Command> for Message {
    fn from(cmd: Command) -> Message {
        Message {
            tags: None,
            prefix: None,
            command: cmd,
        }
    }
}

/// An IRCv3 message tag.
///
/// Tags are key-value pairs that can be attached to messages.
/// The value is optional (some tags are presence-only flags).
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tag(
    /// Tag key (e.g., `time`, `msgid`).
    pub Cow<'static, str>,
    /// Optional tag value.
    pub Option<String>,
);

impl Tag {
    /// Create a new tag with a key and optional value.
    pub fn new(key: impl Into<String>, value: Option<String>) -> Self {
        Tag(Cow::Owned(key.into()), value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;

    #[test]
    fn test_privmsg_constructor() {
        let msg = Message::privmsg("#channel", "Hello, world!");
        match msg.command {
            Command::PRIVMSG(target, text) => {
                assert_eq!(target, "#channel");
                assert_eq!(text, "Hello, world!");
            }
            _ => panic!("Expected PRIVMSG command"),
        }
        assert!(msg.tags.is_none());
        assert!(msg.prefix.is_none());
    }

    #[test]
    fn test_notice_constructor() {
        let msg = Message::notice("nickname", "This is a notice");
        match msg.command {
            Command::NOTICE(target, text) => {
                assert_eq!(target, "nickname");
                assert_eq!(text, "This is a notice");
            }
            _ => panic!("Expected NOTICE command"),
        }
    }

    #[test]
    fn test_join_constructor() {
        let msg = Message::join("#test");
        match msg.command {
            Command::JOIN(channel, key, _) => {
                assert_eq!(channel, "#test");
                assert!(key.is_none());
            }
            _ => panic!("Expected JOIN command"),
        }
    }

    #[test]
    fn test_join_with_key_constructor() {
        let msg = Message::join_with_key("#private", "secret");
        match msg.command {
            Command::JOIN(channel, key, _) => {
                assert_eq!(channel, "#private");
                assert_eq!(key.as_ref().unwrap(), "secret");
            }
            _ => panic!("Expected JOIN command"),
        }
    }

    #[test]
    fn test_part_constructor() {
        let msg = Message::part("#test");
        match msg.command {
            Command::PART(channel, message) => {
                assert_eq!(channel, "#test");
                assert!(message.is_none());
            }
            _ => panic!("Expected PART command"),
        }
    }

    #[test]
    fn test_part_with_message_constructor() {
        let msg = Message::part_with_message("#test", "Goodbye!");
        match msg.command {
            Command::PART(channel, message) => {
                assert_eq!(channel, "#test");
                assert_eq!(message.as_ref().unwrap(), "Goodbye!");
            }
            _ => panic!("Expected PART command"),
        }
    }

    #[test]
    fn test_nick_constructor() {
        let msg = Message::nick("newnick");
        match msg.command {
            Command::NICK(nickname) => {
                assert_eq!(nickname, "newnick");
            }
            _ => panic!("Expected NICK command"),
        }
    }

    #[test]
    fn test_user_constructor() {
        let msg = Message::user("testuser", "Test User");
        match msg.command {
            Command::USER(username, mode, realname) => {
                assert_eq!(username, "testuser");
                assert_eq!(mode, "0");
                assert_eq!(realname, "Test User");
            }
            _ => panic!("Expected USER command"),
        }
    }

    #[test]
    fn test_ping_constructor() {
        let msg = Message::ping("irc.example.com");
        match msg.command {
            Command::PING(server, server2) => {
                assert_eq!(server, "irc.example.com");
                assert!(server2.is_none());
            }
            _ => panic!("Expected PING command"),
        }
    }

    #[test]
    fn test_pong_constructor() {
        let msg = Message::pong("irc.example.com");
        match msg.command {
            Command::PONG(server, server2) => {
                assert_eq!(server, "irc.example.com");
                assert!(server2.is_none());
            }
            _ => panic!("Expected PONG command"),
        }
    }

    #[test]
    fn test_pong_with_token_constructor() {
        let msg = Message::pong_with_token("irc.example.com", "test123");
        match msg.command {
            Command::PONG(server, token) => {
                assert_eq!(server, "irc.example.com");
                assert_eq!(token.as_ref().unwrap(), "test123");
            }
            _ => panic!("Expected PONG command"),
        }
    }

    #[test]
    fn test_quit_constructor() {
        let msg = Message::quit();
        match msg.command {
            Command::QUIT(message) => {
                assert!(message.is_none());
            }
            _ => panic!("Expected QUIT command"),
        }
    }

    #[test]
    fn test_quit_with_message_constructor() {
        let msg = Message::quit_with_message("Goodbye!");
        match msg.command {
            Command::QUIT(message) => {
                assert_eq!(message.as_ref().unwrap(), "Goodbye!");
            }
            _ => panic!("Expected QUIT command"),
        }
    }

    #[test]
    fn test_kick_constructor() {
        let msg = Message::kick("#channel", "baduser");
        match msg.command {
            Command::KICK(channel, nickname, reason) => {
                assert_eq!(channel, "#channel");
                assert_eq!(nickname, "baduser");
                assert!(reason.is_none());
            }
            _ => panic!("Expected KICK command"),
        }
    }

    #[test]
    fn test_kick_with_reason_constructor() {
        let msg = Message::kick_with_reason("#channel", "baduser", "Spam");
        match msg.command {
            Command::KICK(channel, nickname, reason) => {
                assert_eq!(channel, "#channel");
                assert_eq!(nickname, "baduser");
                assert_eq!(reason.as_ref().unwrap(), "Spam");
            }
            _ => panic!("Expected KICK command"),
        }
    }

    #[test]
    fn test_away_constructor() {
        let msg = Message::away();
        match msg.command {
            Command::AWAY(message) => {
                assert!(message.is_none());
            }
            _ => panic!("Expected AWAY command"),
        }
    }

    #[test]
    fn test_away_with_message_constructor() {
        let msg = Message::away_with_message("Be back later");
        match msg.command {
            Command::AWAY(message) => {
                assert_eq!(message.as_ref().unwrap(), "Be back later");
            }
            _ => panic!("Expected AWAY command"),
        }
    }

    #[test]
    fn test_with_message_tags() {
        let msg = Message::privmsg("#test", "Hello").with_message_tags(vec![
            Tag::new("time", Some("2023-01-01T00:00:00Z".to_string())),
            Tag::new("msgid", Some("abc123".to_string())),
        ]);

        assert!(msg.tags.is_some());
        let tags = msg.tags.unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].0.as_ref(), "time");
        assert_eq!(tags[0].1.as_ref().unwrap(), "2023-01-01T00:00:00Z");
        assert_eq!(tags[1].0.as_ref(), "msgid");
        assert_eq!(tags[1].1.as_ref().unwrap(), "abc123");
    }

    #[test]
    fn test_with_tag() {
        let msg = Message::privmsg("#test", "Hello")
            .with_tag("time", Some("2023-01-01T00:00:00Z"))
            .with_tag("msgid", Some("abc123"))
            .with_tag("bot", None::<String>);

        assert!(msg.tags.is_some());
        let tags = msg.tags.unwrap();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].0.as_ref(), "time");
        assert_eq!(tags[0].1.as_ref().unwrap(), "2023-01-01T00:00:00Z");
        assert_eq!(tags[1].0.as_ref(), "msgid");
        assert_eq!(tags[1].1.as_ref().unwrap(), "abc123");
        assert_eq!(tags[2].0.as_ref(), "bot");
        assert!(tags[2].1.is_none());
    }

    #[test]
    fn test_with_prefix() {
        use crate::prefix::Prefix;

        let prefix = Prefix::new_from_str("nick!user@host");
        let msg = Message::privmsg("#test", "Hello").with_prefix(prefix.clone());

        assert!(msg.prefix.is_some());
        assert_eq!(msg.prefix.unwrap(), prefix);
    }

    #[test]
    fn test_chaining_methods() {
        use crate::prefix::Prefix;

        let msg = Message::privmsg("#test", "Hello")
            .with_tag("time", Some("2023-01-01T00:00:00Z"))
            .with_prefix(Prefix::new_from_str("bot!bot@example.com"));

        // Verify command
        match msg.command {
            Command::PRIVMSG(target, text) => {
                assert_eq!(target, "#test");
                assert_eq!(text, "Hello");
            }
            _ => panic!("Expected PRIVMSG command"),
        }

        // Verify tags
        assert!(msg.tags.is_some());
        let tags = msg.tags.unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].0.as_ref(), "time");

        // Verify prefix
        assert!(msg.prefix.is_some());
    }

    #[test]
    fn test_message_round_trip_with_constructors() {
        let original_msg = Message::privmsg("#test", "Hello, world!")
            .with_tag("time", Some("2023-01-01T00:00:00Z"))
            .with_tag("msgid", Some("abc123"));

        let serialized = original_msg.to_string();
        let parsed: Message = serialized.parse().expect("Should parse successfully");

        // The parsed message should be equivalent to the original
        assert_eq!(original_msg, parsed);
    }
}
