//! Connection and registration handlers.
//!
//! Handles NICK, USER, PING, PONG, QUIT commands.

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use crate::state::User;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Response};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Validates an IRC nickname per RFC 2812.
/// First char: letter or special [\]^_`{|}
/// Rest: letter, digit, special, or hyphen
fn is_valid_nick(nick: &str) -> bool {
    if nick.is_empty() || nick.len() > 30 {
        return false;
    }

    let is_special = |c: char| matches!(c, '[' | ']' | '\\' | '`' | '_' | '^' | '{' | '|' | '}');

    let mut chars = nick.chars();
    let first = chars.next().unwrap();
    
    // First char: letter or special
    if !first.is_ascii_alphabetic() && !is_special(first) {
        return false;
    }

    // Rest: letter, digit, special, or hyphen
    chars.all(|c| c.is_ascii_alphanumeric() || is_special(c) || c == '-')
}

/// Handler for NICK command.
pub struct NickHandler;

#[async_trait]
impl Handler for NickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let nick = match &msg.command {
            Command::NICK(n) => n.clone(),
            _ => return Ok(()),
        };

        if nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        if !is_valid_nick(&nick) {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ERRONEOUSNICKNAME,
                vec![
                    ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                    nick.clone(),
                    "Erroneous nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let nick_lower = irc_to_lower(&nick);

        // Check if nick is in use
        if let Some(existing_uid) = ctx.matrix.nicks.get(&nick_lower)
            && existing_uid.value() != ctx.uid
        {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NICKNAMEINUSE,
                vec![
                    ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                    nick.clone(),
                    "Nickname is already in use".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Remove old nick from index if changing
        if let Some(old_nick) = &ctx.handshake.nick {
            let old_nick_lower = irc_to_lower(old_nick);
            ctx.matrix.nicks.remove(&old_nick_lower);
        }

        // Register new nick
        ctx.matrix.nicks.insert(nick_lower, ctx.uid.to_string());
        ctx.handshake.nick = Some(nick.clone());

        debug!(nick = %nick, uid = %ctx.uid, "Nick set");

        // Check if we can complete registration
        if ctx.handshake.can_register() {
            send_welcome_burst(ctx).await?;
        }

        Ok(())
    }
}

/// Handler for USER command.
pub struct UserHandler;

#[async_trait]
impl Handler for UserHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec![
                    ctx.handshake.nick.clone().unwrap_or_else(|| "*".to_string()),
                    "You may not reregister".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let (username, _mode, realname) = match &msg.command {
            Command::USER(u, m, r) => (u.clone(), m.clone(), r.clone()),
            _ => return Ok(()),
        };

        if username.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        ctx.handshake.user = Some(username.clone());
        ctx.handshake.realname = Some(realname.clone());

        debug!(user = %username, realname = %realname, uid = %ctx.uid, "User set");

        // Check if we can complete registration
        if ctx.handshake.can_register() {
            send_welcome_burst(ctx).await?;
        }

        Ok(())
    }
}

/// Send the welcome burst (001-005 + MOTD) after successful registration.
async fn send_welcome_burst(ctx: &mut Context<'_>) -> HandlerResult {
    let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;
    let user = ctx.handshake.user.as_ref().ok_or(HandlerError::NickOrUserMissing)?;
    let realname = ctx.handshake.realname.as_ref().cloned().unwrap_or_default();
    let server_name = &ctx.matrix.server_info.name;
    let network = &ctx.matrix.server_info.network;

    ctx.handshake.registered = true;

    // Create user in Matrix
    let mut user_obj = User::new(
        ctx.uid.to_string(),
        nick.clone(),
        user.clone(),
        realname,
        "localhost".to_string(), // TODO: get actual host
    );

    // Set +r if authenticated via SASL
    if ctx.handshake.account.is_some() {
        user_obj.modes.registered = true;
    }

    ctx.matrix.users.insert(
        ctx.uid.to_string(),
        Arc::new(RwLock::new(user_obj)),
    );

    info!(nick = %nick, user = %user, uid = %ctx.uid, account = ?ctx.handshake.account, "Client registered");

    // 001 RPL_WELCOME
    let welcome = server_reply(
        server_name,
        Response::RPL_WELCOME,
        vec![
            nick.clone(),
            format!(
                "Welcome to the {} IRC Network {}!{}@{}",
                network, nick, user, "localhost"
            ),
        ],
    );
    ctx.sender.send(welcome).await?;

    // 002 RPL_YOURHOST
    let yourhost = server_reply(
        server_name,
        Response::RPL_YOURHOST,
        vec![
            nick.clone(),
            format!("Your host is {}, running version slircd-ng-0.1.0", server_name),
        ],
    );
    ctx.sender.send(yourhost).await?;

    // 003 RPL_CREATED
    let created = server_reply(
        server_name,
        Response::RPL_CREATED,
        vec![
            nick.clone(),
            format!("This server was created at startup"),
        ],
    );
    ctx.sender.send(created).await?;

    // 004 RPL_MYINFO
    // Format: <nick> <servername> <version> <usermodes> <chanmodes>
    let myinfo = server_reply(
        server_name,
        Response::RPL_MYINFO,
        vec![
            nick.clone(),
            server_name.clone(),
            "slircd-ng-0.1.0".to_string(),
            "iowrZ".to_string(),       // user modes: invisible, wallops, oper, registered, secure
            "beIiklmnopqrstv".to_string(), // channel modes
        ],
    );
    ctx.sender.send(myinfo).await?;

    // 005 RPL_ISUPPORT (split into multiple lines to stay under 512 bytes)
    // Line 1: Core parameters
    let isupport1 = server_reply(
        server_name,
        Response::RPL_ISUPPORT,
        vec![
            nick.clone(),
            format!("NETWORK={}", network),
            "CASEMAPPING=rfc1459".to_string(),
            "CHANTYPES=#&+!".to_string(), // All RFC 2811 channel types
            "PREFIX=(ov)@+".to_string(),
            "CHANMODES=beIq,k,l,imnrst".to_string(),
            "are supported by this server".to_string(),
        ],
    );
    ctx.sender.send(isupport1).await?;

    // Line 2: Limits
    let isupport2 = server_reply(
        server_name,
        Response::RPL_ISUPPORT,
        vec![
            nick.clone(),
            "NICKLEN=30".to_string(),
            "CHANNELLEN=50".to_string(),
            "TOPICLEN=390".to_string(),
            "KICKLEN=390".to_string(),
            "AWAYLEN=200".to_string(),
            "MODES=6".to_string(),
            "MAXTARGETS=4".to_string(),
            "are supported by this server".to_string(),
        ],
    );
    ctx.sender.send(isupport2).await?;

    // 375 RPL_MOTDSTART
    let motdstart = server_reply(
        server_name,
        Response::RPL_MOTDSTART,
        vec![
            nick.clone(),
            format!("- {} Message of the Day -", server_name),
        ],
    );
    ctx.sender.send(motdstart).await?;

    // 372 RPL_MOTD
    let motd = server_reply(
        server_name,
        Response::RPL_MOTD,
        vec![nick.clone(), "- Welcome to slircd-ng!".to_string()],
    );
    ctx.sender.send(motd).await?;

    let motd2 = server_reply(
        server_name,
        Response::RPL_MOTD,
        vec![nick.clone(), "- A high-performance IRC daemon.".to_string()],
    );
    ctx.sender.send(motd2).await?;

    // 376 RPL_ENDOFMOTD
    let endmotd = server_reply(
        server_name,
        Response::RPL_ENDOFMOTD,
        vec![nick.clone(), "End of /MOTD command.".to_string()],
    );
    ctx.sender.send(endmotd).await?;

    Ok(())
}

/// Handler for PING command.
pub struct PingHandler;

#[async_trait]
impl Handler for PingHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let server = match &msg.command {
            Command::PING(s, _) => s.clone(),
            _ => return Ok(()),
        };

        let pong = Message::pong(&server);
        ctx.sender.send(pong).await?;

        Ok(())
    }
}

/// Handler for PONG command.
pub struct PongHandler;

#[async_trait]
impl Handler for PongHandler {
    async fn handle(&self, _ctx: &mut Context<'_>, _msg: &Message) -> HandlerResult {
        // Just acknowledge PONG - resets idle timer (handled in connection loop)
        Ok(())
    }
}

/// Handler for QUIT command.
pub struct QuitHandler;

#[async_trait]
impl Handler for QuitHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let quit_msg = match &msg.command {
            Command::QUIT(m) => m.clone(),
            _ => return Ok(()),
        };

        info!(
            uid = %ctx.uid,
            nick = ?ctx.handshake.nick,
            message = ?quit_msg,
            "Client quit"
        );

        // Signal quit by returning an error that connection loop will handle
        Err(HandlerError::NotRegistered) // We'll use a custom error type later
    }
}

/// Handler for PASS command.
///
/// `PASS password`
///
/// Sets the connection password before registration.
pub struct PassHandler;

#[async_trait]
impl Handler for PassHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        // PASS must be sent before NICK/USER
        if ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec!["*".to_string(), "You may not reregister".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let _password = match &msg.command {
            Command::PASS(password) => password.clone(),
            _ => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec!["*".to_string(), "PASS".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // TODO: Store password in handshake state for later validation
        // For now, we accept any password (no server password configured)
        
        debug!("PASS received (not validated - server password not implemented)");

        Ok(())
    }
}
