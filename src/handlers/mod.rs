//! IRC command handlers.
//!
//! This module contains the Handler trait and command registry for dispatching
//! incoming IRC messages to appropriate handlers.

mod channel;
mod connection;
mod messaging;
mod mode;

pub use channel::{JoinHandler, KickHandler, NamesHandler, PartHandler, TopicHandler};
pub use connection::{NickHandler, PingHandler, PongHandler, QuitHandler, UserHandler};
pub use messaging::{NoticeHandler, PrivmsgHandler};
pub use mode::ModeHandler;

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

        // Messaging handlers
        handlers.insert("PRIVMSG", Box::new(PrivmsgHandler));
        handlers.insert("NOTICE", Box::new(NoticeHandler));

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
        Command::NICK(_) => "NICK".to_string(),
        Command::USER(_, _, _) => "USER".to_string(),
        Command::PING(_, _) => "PING".to_string(),
        Command::PONG(_, _) => "PONG".to_string(),
        Command::QUIT(_) => "QUIT".to_string(),
        Command::PRIVMSG(_, _) => "PRIVMSG".to_string(),
        Command::NOTICE(_, _) => "NOTICE".to_string(),
        Command::JOIN(_, _, _) => "JOIN".to_string(),
        Command::PART(_, _) => "PART".to_string(),
        Command::TOPIC(_, _) => "TOPIC".to_string(),
        Command::KICK(_, _, _) => "KICK".to_string(),
        Command::UserMODE(_, _) => "MODE".to_string(),
        Command::ChannelMODE(_, _) => "MODE".to_string(),
        Command::WHO(..) => "WHO".to_string(),
        Command::WHOIS(..) => "WHOIS".to_string(),
        Command::Response(_, _) => "RESPONSE".to_string(),
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

/// Helper to create a server message with a command.
pub fn server_message(server_name: &str, command: Command) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command,
    }
}
