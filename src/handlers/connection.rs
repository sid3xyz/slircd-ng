//! Connection and registration handlers.
//!
//! Handles NICK, USER, PING, PONG, QUIT commands.

use super::{Context, Handler, HandlerError, HandlerResult, server_reply};
use crate::config::WebircBlock;
use crate::state::User;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response, irc_to_lower, wildcard_match};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // NICK <nickname>
        let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        if !is_valid_nick(nick) {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ERRONEOUSNICKNAME,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    nick.to_string(),
                    "Erroneous nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let nick_lower = irc_to_lower(nick);

        // Check if nick is in use
        if let Some(existing_uid) = ctx.matrix.nicks.get(&nick_lower)
            && existing_uid.value() != ctx.uid
        {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NICKNAMEINUSE,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    nick.to_string(),
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
            // Clear any enforcement timer for old nick
            ctx.matrix.enforce_timers.remove(ctx.uid);
        }

        // Register new nick
        ctx.matrix
            .nicks
            .insert(nick_lower.clone(), ctx.uid.to_string());
        ctx.handshake.nick = Some(nick.to_string());

        debug!(nick = %nick, uid = %ctx.uid, "Nick set");

        // Check if nick enforcement should be started
        // Only if user is not already identified to an account
        let is_identified = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
            let user = user.read().await;
            user.modes.registered
        } else {
            false
        };

        if !is_identified {
            // Check if this nick is registered with ENFORCE enabled
            if let Ok(Some(account)) = ctx.db.accounts().find_by_nickname(nick).await
                && account.enforce
            {
                // Start 60 second timer
                let deadline = Instant::now() + Duration::from_secs(60);
                ctx.matrix
                    .enforce_timers
                    .insert(ctx.uid.to_string(), deadline);

                // Notify user
                let notice = Message {
                    tags: None,
                    prefix: Some(Prefix::Nickname(
                        "NickServ".to_string(),
                        "NickServ".to_string(),
                        "services.".to_string(),
                    )),
                    command: Command::NOTICE(
                        nick.to_string(),
                        "This nickname is registered. Please identify via \x02/msg NickServ IDENTIFY <password>\x02 within 60 seconds.".to_string(),
                    ),
                };
                let _ = ctx.sender.send(notice).await;
                info!(nick = %nick, uid = %ctx.uid, "Nick enforcement timer started");
            }
        }

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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec![
                    ctx.handshake
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    "You may not reregister".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // USER <username> <mode> <unused> <realname>
        let username = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        // arg(1) is mode, arg(2) is unused
        let realname = msg.arg(3).unwrap_or("");

        if username.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        ctx.handshake.user = Some(username.to_string());
        ctx.handshake.realname = Some(realname.to_string());

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
    let nick = ctx
        .handshake
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let user = ctx
        .handshake
        .user
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;
    let realname = ctx.handshake.realname.as_ref().cloned().unwrap_or_default();
    let server_name = &ctx.matrix.server_info.name;
    let network = &ctx.matrix.server_info.network;
    let host = ctx.remote_addr.ip().to_string();

    // Check for database bans (K-lines, D-lines, G-lines, Z-lines)
    if let Ok(Some(ban_reason)) = ctx.db.bans().check_all_bans(&host, user, &host).await {
        // ERR_YOUREBANNEDCREEP (465)
        let reply = server_reply(
            server_name,
            Response::ERR_YOUREBANNEDCREEP,
            vec![
                nick.clone(),
                format!("You are banned from this server: {}", ban_reason),
            ],
        );
        ctx.sender.send(reply).await?;

        // Send ERROR and close connection
        let error = Message::from(Command::ERROR(format!(
            "Closing Link: {} ({})",
            host, ban_reason
        )));
        ctx.sender.send(error).await?;

        return Err(HandlerError::NotRegistered);
    }

    // Check for R-line (realname ban)
    if let Ok(Some(ban_reason)) = ctx.db.bans().check_realname_ban(&realname).await {
        let reply = server_reply(
            server_name,
            Response::ERR_YOUREBANNEDCREEP,
            vec![
                nick.clone(),
                format!("You are banned from this server: {}", ban_reason),
            ],
        );
        ctx.sender.send(reply).await?;

        let error = Message::from(Command::ERROR(format!(
            "Closing Link: {} ({})",
            host, ban_reason
        )));
        ctx.sender.send(error).await?;

        return Err(HandlerError::NotRegistered);
    }

    // Check in-memory X-lines (for real-time updates without DB query)
    let user_context = crate::security::UserContext::for_registration(
        ctx.remote_addr.ip(),
        host.clone(),
        nick.clone(),
        user.clone(),
        realname.clone(),
        server_name.clone(),
        ctx.handshake.account.clone(),
    );

    for xline_entry in ctx.matrix.xlines.iter() {
        if crate::security::matches_xline(xline_entry.value(), &user_context) {
            let xline = xline_entry.value();
            let ban_reason = format!("{}: {}", xline.type_name(), xline.reason());

            let reply = server_reply(
                server_name,
                Response::ERR_YOUREBANNEDCREEP,
                vec![
                    nick.clone(),
                    format!("You are banned from this server: {}", ban_reason),
                ],
            );
            ctx.sender.send(reply).await?;

            let error = Message::from(Command::ERROR(format!(
                "Closing Link: {} ({})",
                host, ban_reason
            )));
            ctx.sender.send(error).await?;

            crate::metrics::XLINES_ENFORCED.inc();

            return Err(HandlerError::NotRegistered);
        }
    }

    ctx.handshake.registered = true;

    // Create user in Matrix with cloaking from security config
    let security_config = &ctx.matrix.config.security;
    let mut user_obj = User::new(
        ctx.uid.to_string(),
        nick.clone(),
        user.clone(),
        realname,
        host.clone(),
        &security_config.cloak_secret,
        &security_config.cloak_suffix,
    );

    // Set account and +r if authenticated via SASL
    if let Some(account_name) = &ctx.handshake.account {
        user_obj.modes.registered = true;
        user_obj.account = Some(account_name.clone());
    }

    // Set +Z if TLS connection
    if ctx.handshake.is_tls {
        user_obj.modes.secure = true;
    }

    // Store the cloaked hostname for RPL_HOSTHIDDEN
    let cloaked_host = user_obj.visible_host.clone();

    ctx.matrix
        .users
        .insert(ctx.uid.to_string(), Arc::new(RwLock::new(user_obj)));

    crate::metrics::CONNECTED_USERS.inc();

    info!(nick = %nick, user = %user, uid = %ctx.uid, account = ?ctx.handshake.account, "Client registered");

    // 001 RPL_WELCOME
    // Use cloaked hostname in welcome message
    let welcome = server_reply(
        server_name,
        Response::RPL_WELCOME,
        vec![
            nick.clone(),
            format!(
                "Welcome to the {} IRC Network {}!{}@{}",
                network, nick, user, cloaked_host
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
            format!(
                "Your host is {}, running version slircd-ng-0.1.0",
                server_name
            ),
        ],
    );
    ctx.sender.send(yourhost).await?;

    // 003 RPL_CREATED
    let created = server_reply(
        server_name,
        Response::RPL_CREATED,
        vec![nick.clone(), format!("This server was created at startup")],
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
            "iowrZ".to_string(), // user modes: invisible, wallops, oper, registered, secure
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

    // 396 RPL_HOSTHIDDEN - Notify user that their IP has been cloaked
    let hosthidden = server_reply(
        server_name,
        Response::RPL_HOSTHIDDEN,
        vec![
            nick.clone(),
            cloaked_host.clone(),
            "is now your displayed host".to_string(),
        ],
    );
    ctx.sender.send(hosthidden).await?;

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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // PING <server>
        let server = msg.arg(0).unwrap_or("");

        let pong = Message::pong(server);
        ctx.sender.send(pong).await?;

        Ok(())
    }
}

/// Handler for PONG command.
pub struct PongHandler;

#[async_trait]
impl Handler for PongHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        // PONG normally produces no output, but with labeled-response we send ACK
        if let Some(label) = &ctx.label {
            let ack = super::labeled_ack(&ctx.matrix.server_info.name, label);
            ctx.sender.send(ack).await?;
        }

        // Just acknowledge PONG - resets idle timer (handled in connection loop)
        Ok(())
    }
}

/// Handler for QUIT command.
pub struct QuitHandler;

#[async_trait]
impl Handler for QuitHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let quit_msg = msg.arg(0);

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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
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

        // PASS <password>
        let _password = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![
                        "*".to_string(),
                        "PASS".to_string(),
                        "Not enough parameters".to_string(),
                    ],
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

/// Handler for WEBIRC command.
///
/// `WEBIRC password gateway hostname ip`
///
/// Allows trusted web gateways/proxies to forward real client information.
/// Must be sent before NICK/USER registration.
pub struct WebircHandler {
    /// Configured WEBIRC blocks from server config.
    pub webirc_blocks: Vec<WebircBlock>,
}

impl WebircHandler {
    /// Create a new WebircHandler with the given configuration.
    pub fn new(webirc_blocks: Vec<WebircBlock>) -> Self {
        Self { webirc_blocks }
    }

    /// Check if a WEBIRC request is authorized.
    fn is_authorized(&self, password: &str, gateway_host: &str) -> bool {
        for block in &self.webirc_blocks {
            if block.password == password {
                // If no hosts specified, accept from anywhere
                if block.hosts.is_empty() {
                    return true;
                }
                // Check if gateway_host matches any allowed pattern
                for host_pattern in &block.hosts {
                    if wildcard_match(host_pattern, gateway_host) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[async_trait]
impl Handler for WebircHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // WEBIRC must be sent before registration
        if ctx.handshake.registered || ctx.handshake.nick.is_some() || ctx.handshake.user.is_some()
        {
            // Silently ignore WEBIRC after registration has started
            debug!("WEBIRC rejected: registration already started");
            return Ok(());
        }

        // WEBIRC <password> <gateway> <hostname> <ip>
        let password = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                debug!("WEBIRC rejected: missing password");
                return Ok(());
            }
        };

        let gateway = match msg.arg(1) {
            Some(g) if !g.is_empty() => g,
            _ => {
                debug!("WEBIRC rejected: missing gateway");
                return Ok(());
            }
        };

        let hostname = match msg.arg(2) {
            Some(h) if !h.is_empty() => h,
            _ => {
                debug!("WEBIRC rejected: missing hostname");
                return Ok(());
            }
        };

        let ip = match msg.arg(3) {
            Some(i) if !i.is_empty() => i,
            _ => {
                debug!("WEBIRC rejected: missing IP");
                return Ok(());
            }
        };

        // Get the gateway's connecting IP for authorization check
        let gateway_ip = ctx.remote_addr.ip().to_string();

        // Check authorization
        if !self.is_authorized(password, &gateway_ip) {
            warn!(
                gateway = %gateway,
                gateway_ip = %gateway_ip,
                "WEBIRC rejected: invalid password or unauthorized host"
            );
            // Disconnect the client for security
            let error_msg = Message {
                tags: None,
                prefix: None,
                command: Command::ERROR("WEBIRC authentication failed".to_string()),
            };
            ctx.sender.send(error_msg).await?;
            return Ok(());
        }

        // Store WEBIRC info in handshake state
        ctx.handshake.webirc_used = true;
        ctx.handshake.webirc_ip = Some(ip.to_string());
        ctx.handshake.webirc_host = Some(hostname.to_string());

        info!(
            gateway = %gateway,
            real_ip = %ip,
            real_host = %hostname,
            gateway_ip = %gateway_ip,
            "WEBIRC accepted"
        );

        Ok(())
    }
}
