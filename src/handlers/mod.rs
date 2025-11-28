//! IRC command handlers.
//!
//! This module contains the Handler trait and command registry for dispatching
//! incoming IRC messages to appropriate handlers.

mod admin;
mod bans;
mod channel;
mod connection;
mod messaging;
mod misc;
mod mode;
mod oper;
mod server_query;
mod user_query;

pub use admin::{SajoinHandler, SamodeHandler, SanickHandler, SapartHandler};
pub use bans::{DlineHandler, KlineHandler, UndlineHandler, UnklineHandler};
pub use channel::{JoinHandler, KickHandler, NamesHandler, PartHandler, TopicHandler};
pub use connection::{NickHandler, PassHandler, PingHandler, PongHandler, QuitHandler, UserHandler};
pub use messaging::{NoticeHandler, PrivmsgHandler};
pub use misc::{AwayHandler, InviteHandler, IsonHandler, KnockHandler, UserhostHandler};
pub use mode::ModeHandler;
pub use oper::{DieHandler, KillHandler, OperHandler, RehashHandler, WallopsHandler};
pub use server_query::{
    AdminHandler, InfoHandler, ListHandler, LusersHandler, MotdHandler, StatsHandler,
    TimeHandler, VersionHandler,
};
pub use user_query::{WhoHandler, WhoisHandler, WhowasHandler};

use crate::state::Matrix;
use async_trait::async_trait;
use slirc_proto::{Command, Message, Prefix, Response};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

/// Handler context passed to each command handler.
pub struct Context<'a> {
    /// The user's unique ID.
    pub uid: &'a str,
    /// Shared server state.
    pub matrix: &'a Arc<Matrix>,
    /// Sender for outgoing messages to this client.
    pub sender: &'a mpsc::Sender<Message>,
    /// Current handshake state.
    pub handshake: &'a mut HandshakeState,
}

/// State tracked during client registration handshake.
#[derive(Debug, Default)]
pub struct HandshakeState {
    /// Nick provided by NICK command.
    pub nick: Option<String>,
    /// Username provided by USER command.
    pub user: Option<String>,
    /// Realname provided by USER command.
    pub realname: Option<String>,
    /// Whether registration is complete.
    pub registered: bool,
}

impl HandshakeState {
    /// Check if we have both NICK and USER and can complete registration.
    pub fn can_register(&self) -> bool {
        self.nick.is_some() && self.user.is_some() && !self.registered
    }
}

/// Errors that can occur during command handling.
#[derive(Debug, Error)]
#[allow(dead_code)] // Variants will be used as error handling improves
#[allow(clippy::large_enum_variant)] // Send variant is large but rarely constructed
pub enum HandlerError {
    #[error("not enough parameters")]
    NeedMoreParams,
    #[error("nickname in use: {0}")]
    NicknameInUse(String),
    #[error("erroneous nickname: {0}")]
    ErroneousNickname(String),
    #[error("not registered")]
    NotRegistered,
    #[error("already registered")]
    AlreadyRegistered,
    #[error("send error: {0}")]
    Send(#[from] mpsc::error::SendError<Message>),
}

/// Result type for command handlers.
pub type HandlerResult = Result<(), HandlerError>;

/// Trait implemented by all command handlers.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Handle an incoming message.
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult;
}

/// Registry of command handlers.
pub struct Registry {
    handlers: HashMap<&'static str, Box<dyn Handler>>,
}

impl Registry {
    /// Create a new registry with all handlers registered.
    pub fn new() -> Self {
        let mut handlers: HashMap<&'static str, Box<dyn Handler>> = HashMap::new();

        // Connection/registration handlers
        handlers.insert("NICK", Box::new(NickHandler));
        handlers.insert("USER", Box::new(UserHandler));
        handlers.insert("PASS", Box::new(PassHandler));
        handlers.insert("PING", Box::new(PingHandler));
        handlers.insert("PONG", Box::new(PongHandler));
        handlers.insert("QUIT", Box::new(QuitHandler));

        // Channel handlers
        handlers.insert("JOIN", Box::new(JoinHandler));
        handlers.insert("PART", Box::new(PartHandler));
        handlers.insert("TOPIC", Box::new(TopicHandler));
        handlers.insert("NAMES", Box::new(NamesHandler));
        handlers.insert("MODE", Box::new(ModeHandler));
        handlers.insert("KICK", Box::new(KickHandler));
        handlers.insert("LIST", Box::new(ListHandler));
        handlers.insert("INVITE", Box::new(InviteHandler));

        // Messaging handlers
        handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        handlers.insert("NOTICE", Box::new(NoticeHandler));

        // User query handlers
        handlers.insert("WHO", Box::new(WhoHandler));
        handlers.insert("WHOIS", Box::new(WhoisHandler));
        handlers.insert("WHOWAS", Box::new(WhowasHandler));

        // Server query handlers
        handlers.insert("VERSION", Box::new(VersionHandler));
        handlers.insert("TIME", Box::new(TimeHandler));
        handlers.insert("ADMIN", Box::new(AdminHandler));
        handlers.insert("INFO", Box::new(InfoHandler));
        handlers.insert("LUSERS", Box::new(LusersHandler));
        handlers.insert("STATS", Box::new(StatsHandler));
        handlers.insert("MOTD", Box::new(MotdHandler));

        // Misc handlers
        handlers.insert("AWAY", Box::new(AwayHandler));
        handlers.insert("USERHOST", Box::new(UserhostHandler));
        handlers.insert("ISON", Box::new(IsonHandler));
        handlers.insert("KNOCK", Box::new(KnockHandler));

        // Operator handlers
        handlers.insert("OPER", Box::new(OperHandler));
        handlers.insert("KILL", Box::new(KillHandler));
        handlers.insert("WALLOPS", Box::new(WallopsHandler));
        handlers.insert("DIE", Box::new(DieHandler));
        handlers.insert("REHASH", Box::new(RehashHandler));

        // Ban handlers
        handlers.insert("KLINE", Box::new(KlineHandler));
        handlers.insert("DLINE", Box::new(DlineHandler));
        handlers.insert("UNKLINE", Box::new(UnklineHandler));
        handlers.insert("UNDLINE", Box::new(UndlineHandler));

        // Admin SA* handlers
        handlers.insert("SAJOIN", Box::new(SajoinHandler));
        handlers.insert("SAPART", Box::new(SapartHandler));
        handlers.insert("SANICK", Box::new(SanickHandler));
        handlers.insert("SAMODE", Box::new(SamodeHandler));

        Self { handlers }
    }

    /// Dispatch a message to the appropriate handler.
    pub async fn dispatch(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let cmd_name = command_name(&msg.command);

        if let Some(handler) = self.handlers.get(cmd_name.as_str()) {
            handler.handle(ctx, msg).await
        } else {
            // Unknown command - ignore for now, or send ERR_UNKNOWNCOMMAND
            // For Phase 1, we just ignore unknown commands
            Ok(())
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the command name from a Command enum.
fn command_name(cmd: &Command) -> String {
    match cmd {
        // Connection/registration
        Command::PASS(_) => "PASS".to_string(),
        Command::NICK(_) => "NICK".to_string(),
        Command::USER(..) => "USER".to_string(),
        Command::OPER(..) => "OPER".to_string(),
        Command::QUIT(_) => "QUIT".to_string(),

        // Channel operations
        Command::JOIN(..) => "JOIN".to_string(),
        Command::PART(..) => "PART".to_string(),
        Command::TOPIC(..) => "TOPIC".to_string(),
        Command::NAMES(..) => "NAMES".to_string(),
        Command::LIST(..) => "LIST".to_string(),
        Command::INVITE(..) => "INVITE".to_string(),
        Command::KICK(..) => "KICK".to_string(),
        Command::UserMODE(..) | Command::ChannelMODE(..) => "MODE".to_string(),

        // Messaging
        Command::PRIVMSG(..) => "PRIVMSG".to_string(),
        Command::NOTICE(..) => "NOTICE".to_string(),

        // Server queries
        Command::MOTD(_) => "MOTD".to_string(),
        Command::LUSERS(..) => "LUSERS".to_string(),
        Command::VERSION(_) => "VERSION".to_string(),
        Command::STATS(..) => "STATS".to_string(),
        Command::TIME(_) => "TIME".to_string(),
        Command::ADMIN(_) => "ADMIN".to_string(),
        Command::INFO(_) => "INFO".to_string(),

        // User queries
        Command::WHO(..) => "WHO".to_string(),
        Command::WHOIS(..) => "WHOIS".to_string(),
        Command::WHOWAS(..) => "WHOWAS".to_string(),

        // Miscellaneous
        Command::PING(..) => "PING".to_string(),
        Command::PONG(..) => "PONG".to_string(),
        Command::KILL(..) => "KILL".to_string(),
        Command::AWAY(_) => "AWAY".to_string(),
        Command::REHASH => "REHASH".to_string(),
        Command::DIE => "DIE".to_string(),
        Command::WALLOPS(_) => "WALLOPS".to_string(),
        Command::USERHOST(_) => "USERHOST".to_string(),
        Command::ISON(_) => "ISON".to_string(),

        // Services/admin commands
        Command::SAJOIN(..) => "SAJOIN".to_string(),
        Command::SAPART(..) => "SAPART".to_string(),
        Command::SANICK(..) => "SANICK".to_string(),
        Command::SAMODE(..) => "SAMODE".to_string(),

        // Responses and fallback
        Command::Response(..) => "RESPONSE".to_string(),
        Command::Raw(name, _) => name.to_uppercase(),
        _ => "UNKNOWN".to_string(),
    }
}

/// Helper to create a server reply message.
pub fn server_reply(server_name: &str, response: Response, params: Vec<String>) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::Response(response, params),
    }
}

// ============================================================================
// Common error reply helpers
// ============================================================================

/// Create ERR_NOPRIVILEGES reply (481) - user is not an IRC operator.
pub fn err_noprivileges(server_name: &str, nick: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOPRIVILEGES,
        vec![
            nick.to_string(),
            "Permission Denied - You're not an IRC operator".to_string(),
        ],
    )
}

/// Create ERR_NEEDMOREPARAMS reply (461) - not enough parameters.
pub fn err_needmoreparams(server_name: &str, nick: &str, command: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NEEDMOREPARAMS,
        vec![
            nick.to_string(),
            command.to_string(),
            "Not enough parameters".to_string(),
        ],
    )
}

/// Create ERR_NOSUCHNICK reply (401) - no such nick/channel.
pub fn err_nosuchnick(server_name: &str, nick: &str, target: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOSUCHNICK,
        vec![
            nick.to_string(),
            target.to_string(),
            "No such nick/channel".to_string(),
        ],
    )
}

/// Create ERR_NOSUCHCHANNEL reply (403) - no such channel.
pub fn err_nosuchchannel(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOSUCHCHANNEL,
        vec![
            nick.to_string(),
            channel.to_string(),
            "No such channel".to_string(),
        ],
    )
}

/// Create ERR_NOTONCHANNEL reply (442) - you're not on that channel.
#[allow(dead_code)] // Will be used in future channel.rs refactoring
pub fn err_notonchannel(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOTONCHANNEL,
        vec![
            nick.to_string(),
            channel.to_string(),
            "You're not on that channel".to_string(),
        ],
    )
}

/// Create ERR_CHANOPRIVSNEEDED reply (482) - you're not channel operator.
#[allow(dead_code)] // Will be used in future channel.rs/mode.rs refactoring
pub fn err_chanoprivsneeded(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_CHANOPRIVSNEEDED,
        vec![
            nick.to_string(),
            channel.to_string(),
            "You're not channel operator".to_string(),
        ],
    )
}

/// Create ERR_USERNOTINCHANNEL reply (441) - they aren't on that channel.
#[allow(dead_code)] // Will be used in future channel.rs refactoring
pub fn err_usernotinchannel(server_name: &str, nick: &str, target: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_USERNOTINCHANNEL,
        vec![
            nick.to_string(),
            target.to_string(),
            channel.to_string(),
            "They aren't on that channel".to_string(),
        ],
    )
}

