//! IRC command types.
//!
//! This module provides type-safe representations of IRC commands
//! as defined in RFC 2812 and extended by IRCv3 and modern IRC servers.
//!
//! # Reference
//! - RFC 2812: Internet Relay Chat: Client Protocol
//! - IRCv3 specifications: <https://ircv3.net/>

use crate::mode::{ChannelMode, Mode, UserMode};
use crate::response::Response;
use smallvec::SmallVec;

use super::subcommands::{
    BatchSubCommand, CapSubCommand, ChatHistorySubCommand, MessageReference, MetadataSubCommand,
};

/// IRC command with its parameters.
///
/// This enum represents all known IRC commands with type-safe parameters.
/// Unknown commands are captured in the `Raw` variant.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Command {
    // === Connection Registration (RFC 2812 Section 3.1) ===
    /// `PASS password`
    PASS(String),
    /// TS6 server-to-server handshake form: `PASS <password> TS 6 :<sid>`
    PassTs6 {
        /// Shared link password.
        password: String,
        /// Server ID (SID) advertising the sender.
        sid: String,
    },
    /// `NICK nickname`
    NICK(String),
    /// `USER username mode realname`
    USER(String, String, String),
    /// `OPER name password`
    OPER(String, String),
    /// User MODE command: `MODE nickname [modes]`
    UserMODE(String, Vec<Mode<UserMode>>),
    /// `SERVICE nickname reserved distribution type reserved info`
    SERVICE(String, String, String, String, String, String),
    /// `QUIT [message]`
    QUIT(Option<String>),
    /// `SQUIT server comment`
    SQUIT(String, String),

    // === Channel Operations (RFC 2812 Section 3.2) ===
    /// `JOIN channels [keys] [realname]`
    JOIN(String, Option<String>, Option<String>),
    /// `PART channels [message]`
    PART(String, Option<String>),
    /// Channel MODE command: `MODE channel [modes]`
    ChannelMODE(String, Vec<Mode<ChannelMode>>),
    /// `TOPIC channel [topic]`
    TOPIC(String, Option<String>),
    /// `NAMES [channels] [target]`
    NAMES(Option<String>, Option<String>),
    /// `LIST [channels] [target]`
    LIST(Option<String>, Option<String>),
    /// `INVITE nickname channel`
    INVITE(String, String),
    /// `KICK channels users [comment]`
    KICK(String, String, Option<String>),

    // === Messaging (RFC 2812 Section 3.3) ===
    /// `PRIVMSG target text`
    PRIVMSG(String, String),
    /// `NOTICE target text`
    NOTICE(String, String),
    /// `ACCEPT [nicknames]` - Caller ID (allow/deny list)
    ACCEPT(String),

    // === Server Queries (RFC 2812 Section 3.4) ===
    /// `MOTD [target]`
    MOTD(Option<String>),
    /// `LUSERS [mask] [target]`
    LUSERS(Option<String>, Option<String>),
    /// `VERSION [target]`
    VERSION(Option<String>),
    /// `STATS [query] [target]`
    STATS(Option<String>, Option<String>),
    /// `LINKS [[remote] mask]`
    LINKS(Option<String>, Option<String>),
    /// `TIME [target]`
    TIME(Option<String>),
    /// `CONNECT target port [remote]`
    CONNECT(String, String, Option<String>),
    /// `TRACE [target]`
    TRACE(Option<String>),
    /// `ADMIN [target]`
    ADMIN(Option<String>),
    /// `INFO [target]`
    INFO(Option<String>),
    /// `MAP` - Display server map (network topology)
    MAP,
    /// `RULES` - Display server rules
    RULES,
    /// `USERIP nicknames...` - Get IP addresses of users (oper-only)
    USERIP(Vec<String>),
    /// `HELP [subject]` - Request help on a command or topic
    HELP(Option<String>),
    /// `METADATA subcommand target [params...]` - Get/set user or channel metadata (Ergo)
    /// Subcommands: GET, SET, LIST
    METADATA {
        /// The METADATA subcommand (GET, SET, LIST).
        subcommand: MetadataSubCommand,
        /// The target (nick or channel) to operate on.
        target: String,
        /// For GET: the key to retrieve.
        /// For SET: the key to set, followed by value (or empty for deletion).
        /// For LIST: ignored.
        params: Vec<String>,
    },

    // === Server-to-Server (S2S) ===
    /// `SID name hopcount sid description` - Server introduction
    SID(String, String, String, String),
    /// `CAPAB capabilities...` - Server capability negotiation
    CAPAB(Vec<String>),
    /// `SVINFO version min_version 0 :current_time` - Server version info
    SVINFO(u32, u32, u32, u64),
    /// `UID nick hopcount timestamp username hostname uid modes realname` - User introduction
    UID(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ),
    /// `SJOIN ts channel modes [args...] :users` - Timestamped channel join
    /// Users are stored as (prefixes, uid) tuples.
    SJOIN(u64, String, String, Vec<String>, Vec<(String, String)>),
    /// `TMODE ts channel modes [args...]` - Timestamped mode change
    TMODE(u64, String, String, Vec<String>),
    /// `ENCAP target subcommand [params...]` - Encapsulated server-to-server command
    ENCAP(String, String, Vec<String>),

    // === Service Queries (RFC 2812 Section 3.5) ===
    /// `SERVLIST [mask] [type]`
    SERVLIST(Option<String>, Option<String>),
    /// `SQUERY servicename text`
    SQUERY(String, String),

    // === User Queries (RFC 2812 Section 3.6) ===
    /// `WHO [mask] [flags]`
    ///
    /// The second parameter can be:
    /// - `"o"` for RFC 2812 operators-only flag
    /// - `"%<fields>[,<token>]"` for IRCv3 WHOX extended query
    WHO(Option<String>, Option<String>),
    /// `WHOIS [target] nickmasks`
    WHOIS(Option<String>, String),
    /// `WHOWAS nickname [count] [target]`
    WHOWAS(String, Option<String>, Option<String>),

    // === Miscellaneous (RFC 2812 Section 3.7) ===
    /// `KILL nickname comment`
    KILL(String, String),
    /// `PING server1 [server2]`
    PING(String, Option<String>),
    /// `PONG server1 [server2]`
    PONG(String, Option<String>),
    /// `ERROR message`
    ERROR(String),

    // === Optional Features (RFC 2812 Section 4) ===
    /// `AWAY [message]`
    AWAY(Option<String>),
    /// `REHASH` (no parameters)
    REHASH,
    /// `DIE` (no parameters)
    DIE,
    /// `RESTART` (no parameters)
    RESTART,
    /// `SUMMON user [target] [channel]`
    SUMMON(String, Option<String>, Option<String>),
    /// `USERS [target]`
    USERS(Option<String>),
    /// `WALLOPS text`
    WALLOPS(String),
    /// `GLOBOPS text`
    GLOBOPS(String),
    /// `USERHOST nicknames...`
    USERHOST(Vec<String>),
    /// `ISON nicknames...`
    ISON(Vec<String>),

    // === Operator Ban Commands ===
    /// `KLINE [time] user@host :reason`
    KLINE(Option<String>, String, String),
    /// `DLINE [time] host :reason`
    DLINE(Option<String>, String, String),
    /// `UNKLINE user@host`
    UNKLINE(String),
    /// `UNDLINE host`
    UNDLINE(String),
    /// `GLINE mask [reason]` - Global K-line (network-wide user@host ban)
    GLINE(String, Option<String>),
    /// `UNGLINE mask` - Remove global K-line
    UNGLINE(String),
    /// `ZLINE ip [reason]` - Global IP ban (network-wide IP-based ban)
    ZLINE(String, Option<String>),
    /// `UNZLINE ip` - Remove global IP ban
    UNZLINE(String),
    /// `RLINE pattern [reason]` - Realname/GECOS ban (ban by realname pattern)
    RLINE(String, Option<String>),
    /// `UNRLINE pattern` - Remove realname ban
    UNRLINE(String),
    /// `SHUN mask [reason]` - Silent ignore (silently drop messages from user)
    SHUN(String, Option<String>),
    /// `UNSHUN mask` - Remove shun
    UNSHUN(String),

    // === Channel Extension Commands ===
    /// `KNOCK channel [:message]`
    KNOCK(String, Option<String>),

    // === Server-to-Server (Distributed) ===
    /// `SERVER servername hopcount token info`
    SERVER(String, u32, String, String),

    // === Services Commands (common extensions) ===
    /// `SAJOIN nick channel`
    SAJOIN(String, String),
    /// `SAMODE target modes [params]`
    SAMODE(String, String, Option<String>),
    /// `SANICK oldnick newnick`
    SANICK(String, String),
    /// `SAPART nick channel`
    SAPART(String, String),
    /// `SAQUIT nick reason`
    SAQUIT(String, String),
    /// NickServ shorthand: `NICKSERV params...`
    NICKSERV(Vec<String>),
    /// ChanServ shorthand: `CHANSERV params...`
    CHANSERV(Vec<String>),
    /// OperServ shorthand: `OPERSERV params...`
    OPERSERV(Vec<String>),
    /// BotServ shorthand: `BOTSERV params...`
    BOTSERV(Vec<String>),
    /// HostServ shorthand: `HOSTSERV params...`
    HOSTSERV(Vec<String>),
    /// MemoServ shorthand: `MEMOSERV params...`
    MEMOSERV(Vec<String>),
    /// NickServ alias: `NS params...`
    NS(Vec<String>),
    /// ChanServ alias: `CS params...`
    CS(Vec<String>),
    /// OperServ alias: `OS params...`
    OS(Vec<String>),
    /// BotServ alias: `BS params...`
    BS(Vec<String>),
    /// HostServ alias: `HS params...`
    HS(Vec<String>),
    /// MemoServ alias: `MS params...`
    MS(Vec<String>),

    // === IRCv3 Extensions ===
    /// `CAP [target] subcommand [params] [capabilities]`
    CAP(
        Option<String>,
        CapSubCommand,
        Option<String>,
        Option<String>,
    ),
    /// `AUTHENTICATE mechanism_or_data`
    AUTHENTICATE(String),
    /// `ACCOUNT accountname`
    ACCOUNT(String),
    /// `MONITOR +/-/C/L/S [targets]`
    MONITOR(String, Option<String>),
    /// `BATCH +/-reference [type] [params...]`
    BATCH(String, Option<BatchSubCommand>, Option<Vec<String>>),
    /// `CHGHOST user host`
    CHGHOST(String, String),
    /// `CHGIDENT nick ident`
    CHGIDENT(String, String),
    /// `SETNAME realname`
    SETNAME(String),
    /// `TAGMSG target` - IRCv3 message-tags: message with only tags, no text
    TAGMSG(String),
    /// `ACK` - IRCv3 labeled-response: acknowledgment for labeled commands with no output
    ACK,
    /// `WEBIRC password gateway hostname ip [:options]` - WebIRC/CGI:IRC identification
    WEBIRC(String, String, String, String, Option<String>),
    /// `CHATHISTORY subcommand target/params...` - IRCv3 chat history retrieval
    ///
    /// Variants:
    /// - `LATEST <target> <* | msgref> <limit>`
    /// - `BEFORE/AFTER/AROUND <target> <msgref> <limit>`
    /// - `BETWEEN <target> <msgref> <msgref> <limit>`
    /// - `TARGETS <timestamp> <timestamp> <limit>`
    CHATHISTORY {
        /// The CHATHISTORY subcommand (LATEST, BEFORE, AFTER, etc.).
        subcommand: ChatHistorySubCommand,
        /// Target channel or `*` for all targets.
        target: String,
        /// First message reference or timestamp.
        msg_ref1: MessageReference,
        /// Second message reference (for BETWEEN).
        msg_ref2: Option<MessageReference>,
        /// Maximum number of messages to return.
        limit: u32,
    },
    /// `CHATHISTORY TARGETS target timestamp` - Response to CHATHISTORY TARGETS request
    ChatHistoryTargets {
        /// The target (channel/user).
        target: String,
        /// The timestamp of the latest message (RFC3339 format).
        timestamp: String,
    },
    /// `NPC channel nick :text` - ROLEPLAY: Send message as another character (Ergo)
    NPC {
        /// The channel to send the message to.
        channel: String,
        /// The nick to send the message as.
        nick: String,
        /// The message text.
        text: String,
    },
    /// `RELAYMSG relay_from target text` - Relay message between networks (Ergo)
    RELAYMSG {
        /// The original sender (network/server/nick).
        relay_from: String,
        /// Target of the relayed message.
        target: String,
        /// The relayed message text.
        text: String,
    },

    // === Standard Replies (IRCv3) ===
    /// `FAIL command code [context...] :description`
    FAIL(String, String, Vec<String>),
    /// `WARN command code [context...] :description`
    WARN(String, String, Vec<String>),
    /// `NOTE command code [context...] :description`
    NOTE(String, String, Vec<String>),
    /// `REGISTER SUCCESS account [:<message>]` - Account registration success (IRCv3)
    REGISTER {
        /// Account name that was registered
        account: String,
        /// Optional success message
        message: Option<String>,
    },

    // === Numeric Response ===
    /// Numeric response from server
    Response(Response, Vec<String>),

    // === Unknown/Raw Commands ===
    /// Unknown command captured as raw
    Raw(String, Vec<String>),
}

/// A borrowed reference to a command.
///
/// Used for zero-copy parsing of IRC messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRef<'a> {
    /// Command name
    pub name: &'a str,
    /// Command arguments
    pub args: SmallVec<[&'a str; 15]>,
}

impl Command {
    /// Get the command name as a static string.
    ///
    /// Returns the IRC command name (e.g., "PRIVMSG", "NICK", "JOIN").
    /// For `Response` variants, returns "RESPONSE".
    /// For `Raw` variants, this allocates - prefer using `Command::raw_name()` for those.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::Command;
    ///
    /// let cmd = Command::PRIVMSG("#channel".into(), "Hello".into());
    /// assert_eq!(cmd.name(), "PRIVMSG");
    ///
    /// let cmd = Command::NICK("user".into());
    /// assert_eq!(cmd.name(), "NICK");
    /// ```
    #[inline]
    pub fn name(&self) -> &'static str {
        match self {
            // Connection Registration
            Command::PASS(_) => "PASS",
            Command::PassTs6 { .. } => "PASS",
            Command::NICK(_) => "NICK",
            Command::USER(..) => "USER",
            Command::OPER(..) => "OPER",
            Command::UserMODE(..) => "MODE",
            Command::SERVICE(..) => "SERVICE",
            Command::QUIT(_) => "QUIT",
            Command::SQUIT(..) => "SQUIT",

            // Channel Operations
            Command::JOIN(..) => "JOIN",
            Command::PART(..) => "PART",
            Command::ChannelMODE(..) => "MODE",
            Command::TOPIC(..) => "TOPIC",
            Command::NAMES(..) => "NAMES",
            Command::LIST(..) => "LIST",
            Command::INVITE(..) => "INVITE",
            Command::KICK(..) => "KICK",

            // Messaging
            Command::PRIVMSG(..) => "PRIVMSG",
            Command::NOTICE(..) => "NOTICE",
            Command::ACCEPT(_) => "ACCEPT",

            // Server Queries
            Command::MOTD(_) => "MOTD",
            Command::LUSERS(..) => "LUSERS",
            Command::VERSION(_) => "VERSION",
            Command::STATS(..) => "STATS",
            Command::LINKS(..) => "LINKS",
            Command::TIME(_) => "TIME",
            Command::CONNECT(..) => "CONNECT",
            Command::TRACE(_) => "TRACE",
            Command::ADMIN(_) => "ADMIN",
            Command::INFO(_) => "INFO",
            Command::MAP => "MAP",
            Command::RULES => "RULES",
            Command::USERIP(_) => "USERIP",
            Command::HELP(_) => "HELP",
            Command::METADATA { .. } => "METADATA",

            // Service Queries
            Command::SERVLIST(..) => "SERVLIST",
            Command::SQUERY(..) => "SQUERY",

            // User Queries
            Command::WHO(..) => "WHO",
            Command::WHOIS(..) => "WHOIS",
            Command::WHOWAS(..) => "WHOWAS",

            // Miscellaneous
            Command::KILL(..) => "KILL",
            Command::PING(..) => "PING",
            Command::PONG(..) => "PONG",
            Command::ERROR(_) => "ERROR",

            // Optional Features
            Command::AWAY(_) => "AWAY",
            Command::REHASH => "REHASH",
            Command::DIE => "DIE",
            Command::RESTART => "RESTART",
            Command::SUMMON(..) => "SUMMON",
            Command::USERS(_) => "USERS",
            Command::WALLOPS(_) => "WALLOPS",
            Command::GLOBOPS(_) => "GLOBOPS",
            Command::USERHOST(_) => "USERHOST",
            Command::ISON(_) => "ISON",

            // Operator Ban Commands
            Command::KLINE(..) => "KLINE",
            Command::DLINE(..) => "DLINE",
            Command::UNKLINE(_) => "UNKLINE",
            Command::UNDLINE(_) => "UNDLINE",
            Command::GLINE(..) => "GLINE",
            Command::UNGLINE(_) => "UNGLINE",
            Command::ZLINE(..) => "ZLINE",
            Command::UNZLINE(_) => "UNZLINE",
            Command::RLINE(..) => "RLINE",
            Command::UNRLINE(_) => "UNRLINE",
            Command::SHUN(..) => "SHUN",
            Command::UNSHUN(_) => "UNSHUN",

            // Channel Extensions
            Command::KNOCK(..) => "KNOCK",

            // Server-to-Server
            Command::SERVER(..) => "SERVER",
            Command::SID(..) => "SID",
            Command::UID(..) => "UID",
            Command::SJOIN(..) => "SJOIN",
            Command::TMODE(..) => "TMODE",
            Command::ENCAP(..) => "ENCAP",
            Command::CAPAB(..) => "CAPAB",
            Command::SVINFO(..) => "SVINFO",

            // Services Commands
            Command::SAJOIN(..) => "SAJOIN",
            Command::SAMODE(..) => "SAMODE",
            Command::SANICK(..) => "SANICK",
            Command::SAPART(..) => "SAPART",
            Command::SAQUIT(..) => "SAQUIT",
            Command::NICKSERV(_) => "NICKSERV",
            Command::CHANSERV(_) => "CHANSERV",
            Command::OPERSERV(_) => "OPERSERV",
            Command::BOTSERV(_) => "BOTSERV",
            Command::HOSTSERV(_) => "HOSTSERV",
            Command::MEMOSERV(_) => "MEMOSERV",
            Command::NS(_) => "NS",
            Command::CS(_) => "CS",
            Command::OS(_) => "OS",
            Command::BS(_) => "BS",
            Command::HS(_) => "HS",
            Command::MS(_) => "MS",

            // IRCv3 Extensions
            Command::CAP(..) => "CAP",
            Command::AUTHENTICATE(_) => "AUTHENTICATE",
            Command::ACCOUNT(_) => "ACCOUNT",
            Command::MONITOR(..) => "MONITOR",
            Command::BATCH(..) => "BATCH",
            Command::CHGHOST(..) => "CHGHOST",
            Command::CHGIDENT(..) => "CHGIDENT",
            Command::SETNAME(_) => "SETNAME",
            Command::TAGMSG(_) => "TAGMSG",
            Command::ACK => "ACK",
            Command::WEBIRC(..) => "WEBIRC",
            Command::CHATHISTORY { .. } => "CHATHISTORY",
            Command::ChatHistoryTargets { .. } => "CHATHISTORY",
            Command::NPC { .. } => "NPC",
            Command::RELAYMSG { .. } => "RELAYMSG",

            // Standard Replies
            Command::FAIL(..) => "FAIL",
            Command::WARN(..) => "WARN",
            Command::NOTE(..) => "NOTE",
            Command::REGISTER { .. } => "REGISTER",

            // Numeric Response
            Command::Response(..) => "RESPONSE",

            // Raw - returns "RAW", use raw_name() for the actual command
            Command::Raw(..) => "RAW",
        }
    }

    /// Get the raw command name for `Raw` variants.
    ///
    /// Returns `Some(&str)` for `Raw` commands, `None` for typed commands.
    /// Use `name()` for typed commands.
    #[inline]
    pub fn raw_name(&self) -> Option<&str> {
        match self {
            Command::Raw(name, _) => Some(name),
            _ => None,
        }
    }
}

impl<'a> CommandRef<'a> {
    /// Create a new command reference.
    pub fn new(name: &'a str, args: SmallVec<[&'a str; 15]>) -> Self {
        Self { name, args }
    }

    /// Convert this reference to an owned raw command string.
    pub fn to_raw_string(&self) -> String {
        if self.args.is_empty() {
            self.name.to_string()
        } else {
            let capacity =
                self.name.len() + 1 + self.args.iter().map(|a| a.len() + 1).sum::<usize>();
            let mut s = String::with_capacity(capacity);
            s.push_str(self.name);
            s.push(' ');
            s.push_str(&self.args.join(" "));
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_ref_to_raw_string() {
        let cmd = CommandRef::new("PRIVMSG", SmallVec::from(vec!["#channel", "hello"]));
        assert_eq!(cmd.to_raw_string(), "PRIVMSG #channel hello");

        let cmd = CommandRef::new("PING", SmallVec::from(vec![]));
        assert_eq!(cmd.to_raw_string(), "PING");
    }

    #[test]
    fn test_command_equality() {
        let cmd1 = Command::NICK("test".to_string());
        let cmd2 = Command::NICK("test".to_string());
        assert_eq!(cmd1, cmd2);

        let cmd3 = Command::NICK("other".to_string());
        assert_ne!(cmd1, cmd3);
    }
}
