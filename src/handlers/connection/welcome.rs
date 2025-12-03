//! Welcome burst and registration completion.

use super::super::{Context, HandlerError, HandlerResult, notify_monitors_online, server_reply};
use crate::state::User;
use slirc_proto::{Command, Message, Response};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Send the welcome burst (001-005 + MOTD) after successful registration.
pub async fn send_welcome_burst(ctx: &mut Context<'_>) -> HandlerResult {
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

    // Check server password if configured
    if let Some(required_password) = &ctx.matrix.config.server.password {
        match &ctx.handshake.pass_received {
            None => {
                // No password provided but one is required
                let reply = server_reply(
                    server_name,
                    Response::ERR_PASSWDMISMATCH,
                    vec![nick.clone(), "Password required".to_string()],
                );
                ctx.sender.send(reply).await?;
                let error = Message::from(Command::ERROR(
                    "Closing Link: Access denied (password required)".to_string(),
                ));
                ctx.sender.send(error).await?;
                return Err(HandlerError::AccessDenied);
            }
            Some(provided) if provided != required_password => {
                // Wrong password
                let reply = server_reply(
                    server_name,
                    Response::ERR_PASSWDMISMATCH,
                    vec![nick.clone(), "Password incorrect".to_string()],
                );
                ctx.sender.send(reply).await?;
                let error = Message::from(Command::ERROR(
                    "Closing Link: Access denied (bad password)".to_string(),
                ));
                ctx.sender.send(error).await?;
                return Err(HandlerError::AccessDenied);
            }
            Some(_) => {
                // Password correct, continue
            }
        }
    }

    // Check BanCache for user@host bans (G-lines, K-lines) - fast in-memory check
    if let Some(ban_result) = ctx.matrix.ban_cache.check_user_host(user, &host) {
        let ban_reason = format!("{}: {}", ban_result.ban_type, ban_result.reason);

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

    // Fallback: Check database for user@host bans (G-lines, K-lines)
    // IP bans (Z-lines, D-lines) already checked at gateway by IpDenyList
    if let Ok(Some(ban_reason)) = ctx.db.bans().check_user_host_bans(user, &host).await {
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
        ctx.handshake.capabilities.clone(),
        ctx.handshake.certfp.clone(),
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
            // CHANMODES=A,B,C,D format:
            // A=list modes, B=always param, C=param when set, D=no param
            // Added f,L,j,J to C (param when setting)
            "CHANMODES=beIq,k,fLjJl,imnrst".to_string(),
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

    // Notify MONITOR watchers that this nick has come online
    notify_monitors_online(ctx.matrix, nick, user, &cloaked_host).await;

    Ok(())
}
