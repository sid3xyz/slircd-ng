//! Connection and registration handlers.
//!
//! Handles NICK, USER, PING, PONG, QUIT commands.

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Response};
use tracing::{debug, info};

/// Validates an IRC nickname.
fn is_valid_nick(nick: &str) -> bool {
    if nick.is_empty() || nick.len() > 30 {
        return false;
    }

    let first = nick.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' && !matches!(first, '[' | ']' | '\\' | '`' | '^' | '{' | '}' | '|') {
        return false;
    }

    nick.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || c == '-'
            || matches!(c, '[' | ']' | '\\' | '`' | '^' | '{' | '}' | '|')
    })
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
                Response::ERR_ALREADYREGISTRED,
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
    let nick = ctx.handshake.nick.as_ref().unwrap();
    let user = ctx.handshake.user.as_ref().unwrap();
    let server_name = &ctx.matrix.server_info.name;
    let network = &ctx.matrix.server_info.network;

    ctx.handshake.registered = true;

    info!(nick = %nick, user = %user, uid = %ctx.uid, "Client registered");

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
    let myinfo = server_reply(
        server_name,
        Response::RPL_MYINFO,
        vec![
            nick.clone(),
            server_name.clone(),
            "slircd-ng-0.1.0".to_string(),
            "iowZ".to_string(),      // user modes
            "biklmnopstv".to_string(), // channel modes
        ],
    );
    ctx.sender.send(myinfo).await?;

    // 005 RPL_ISUPPORT
    let isupport = server_reply(
        server_name,
        Response::RPL_ISUPPORT,
        vec![
            nick.clone(),
            format!("NETWORK={}", network),
            "CASEMAPPING=rfc1459".to_string(),
            "NICKLEN=30".to_string(),
            "CHANNELLEN=50".to_string(),
            "are supported by this server".to_string(),
        ],
    );
    ctx.sender.send(isupport).await?;

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
