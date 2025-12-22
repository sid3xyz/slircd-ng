//! Welcome burst writer - sends registration completion directly to transport.
//!
//! This module provides `WelcomeBurstWriter` which handles the welcome burst
//! (001-005 + MOTD) by writing directly to the transport. This avoids the
//! bounded channel deadlock that occurs when MOTD files exceed channel capacity.
//!
//! # Design Rationale
//!
//! The welcome burst is NOT a regular handler - it's infrastructure code that
//! runs at a specific state transition point. Unlike handlers which queue
//! messages for later delivery, the welcome burst writes synchronously to
//! the wire because:
//!
//! 1. It runs in a single-writer context (no concurrent senders)
//! 2. It must complete atomically before entering the main loop
//! 3. MOTD files can be arbitrarily large (100+ lines is common)
//!
//! Writing directly to transport avoids intermediate buffering and ensures
//! the welcome burst completes regardless of MOTD size.

use crate::db::Database;
use crate::error::{HandlerError, HandlerResult};
use crate::handlers::{notify_monitors_online, server_reply};
use crate::state::{Matrix, UnregisteredState, User};
use slirc_proto::isupport::{ChanModesBuilder, IsupportBuilder, TargMaxBuilder};
use slirc_proto::transport::ZeroCopyTransportEnum;
use slirc_proto::{Command, Message, Prefix, Response};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

/// Writer that sends welcome burst directly to transport.
///
/// This is separate from the regular handler infrastructure because:
/// - Handlers use ResponseMiddleware (buffered or capturing)
/// - Welcome burst must write directly to avoid channel deadlock
pub struct WelcomeBurstWriter<'a> {
    uid: &'a str,
    matrix: &'a Arc<Matrix>,
    transport: &'a mut ZeroCopyTransportEnum,
    state: &'a UnregisteredState,
    db: &'a Database,
    remote_addr: SocketAddr,
}

impl<'a> WelcomeBurstWriter<'a> {
    /// Create a new welcome burst writer.
    pub fn new(
        uid: &'a str,
        matrix: &'a Arc<Matrix>,
        transport: &'a mut ZeroCopyTransportEnum,
        state: &'a UnregisteredState,
        db: &'a Database,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            uid,
            matrix,
            transport,
            state,
            db,
            remote_addr,
        }
    }

    /// Write a message directly to the transport.
    async fn write(&mut self, msg: Message) -> HandlerResult {
        self.transport
            .write_message(&msg)
            .await
            .map_err(|e| HandlerError::Internal(format!("transport write error: {e}")))
    }

    /// Send the complete welcome burst.
    ///
    /// Returns `Ok(())` on success, or an error if the client should be disconnected
    /// (e.g., banned, wrong password).
    pub async fn send(mut self) -> HandlerResult {
        let nick = self
            .state
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = self
            .state
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let realname = self.state.realname.as_ref().cloned().unwrap_or_default();
        let server_name = &self.matrix.server_info.name;
        let network = &self.matrix.server_info.network;
        let remote_ip = self.remote_addr.ip().to_string();

        // Record successful connection for reputation
        if let Some(spam_lock) = &self.matrix.security_manager.spam_detector {
            let spam = spam_lock.read().await;
            spam.record_connection_success(self.remote_addr.ip()).await;
        }

        let webirc_ip = self.state.webirc_ip.clone();
        let webirc_host = self.state.webirc_host.clone();

        // Prefer WEBIRC-provided host/IP when available (trusted gateway path)
        let ban_host = webirc_host
            .clone()
            .or(webirc_ip.clone())
            .unwrap_or_else(|| remote_ip.clone());
        let host = ban_host.clone();

        // Check server password if configured
        if let Some(required_password) = &self.matrix.config.server.password {
            match &self.state.pass_received {
                None => {
                    let reply = Response::err_passwdmismatch(nick)
                        .with_prefix(Prefix::ServerName(server_name.to_string()));
                    self.write(reply).await?;
                    let error = Message::from(Command::ERROR(
                        "Closing Link: Access denied (password required)".to_string(),
                    ));
                    self.write(error).await?;
                    return Err(HandlerError::AccessDenied);
                }
                Some(provided) if provided != required_password => {
                    let reply = Response::err_passwdmismatch(nick)
                        .with_prefix(Prefix::ServerName(server_name.to_string()));
                    self.write(reply).await?;
                    let error = Message::from(Command::ERROR(
                        "Closing Link: Access denied (bad password)".to_string(),
                    ));
                    self.write(error).await?;
                    return Err(HandlerError::AccessDenied);
                }
                Some(_) => {}
            }
        }

        // Check BanCache for user@host bans (G-lines, K-lines)
        if let Some(ban_result) = self
            .matrix
            .security_manager
            .ban_cache
            .check_user_host(user, &host)
        {
            let ban_reason = format!("{}: {}", ban_result.ban_type, ban_result.reason);
            let reply = Response::err_yourebannedcreep(nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            self.write(reply).await?;
            let error = Message::from(Command::ERROR(format!(
                "Closing Link: {} ({})",
                host, ban_reason
            )));
            self.write(error).await?;
            return Err(HandlerError::AccessDenied);
        }

        // Fallback: Check database for user@host bans
        if let Ok(Some(ban_reason)) = self.db.bans().check_user_host_bans(user, &host).await {
            let reply = server_reply(
                server_name,
                Response::ERR_YOUREBANNEDCREEP,
                vec![
                    nick.clone(),
                    format!("You are banned from this server: {}", ban_reason),
                ],
            );
            self.write(reply).await?;
            let error = Message::from(Command::ERROR(format!(
                "Closing Link: {} ({})",
                host, ban_reason
            )));
            self.write(error).await?;
            return Err(HandlerError::AccessDenied);
        }

        // Check for R-line (realname ban)
        if let Ok(Some(ban_reason)) = self.db.bans().check_realname_ban(&realname).await {
            let reply = server_reply(
                server_name,
                Response::ERR_YOUREBANNEDCREEP,
                vec![
                    nick.clone(),
                    format!("You are banned from this server: {}", ban_reason),
                ],
            );
            self.write(reply).await?;
            let error = Message::from(Command::ERROR(format!(
                "Closing Link: {} ({})",
                host, ban_reason
            )));
            self.write(error).await?;
            return Err(HandlerError::AccessDenied);
        }

        // Create user in Matrix
        let security_config = &self.matrix.config.security;
        let ip = webirc_ip.clone().unwrap_or_else(|| remote_ip.clone());
        let mut user_obj = User::new(crate::state::UserParams {
            uid: self.uid.to_string(),
            nick: nick.clone(),
            user: user.clone(),
            realname,
            host: host.clone(),
            ip,
            cloak_secret: security_config.cloak_secret.clone(),
            cloak_suffix: security_config.cloak_suffix.clone(),
            caps: self.state.capabilities.clone(),
            certfp: self.state.certfp.clone(),
            last_modified: self.matrix.clock(),
        });

        // Set account and +r if authenticated via SASL
        if let Some(account_name) = &self.state.account {
            user_obj.modes.registered = true;
            user_obj.account = Some(account_name.clone());
        }

        // Set +Z if TLS connection
        if self.state.is_tls {
            user_obj.modes.secure = true;
        }

        let cloaked_host = user_obj.visible_host.clone();

        self.matrix.user_manager.add_local_user(user_obj).await;

        crate::metrics::CONNECTED_USERS.inc();

        // Use real_user_count to exclude service pseudoclients from max tracking
        let current_count = self.matrix.user_manager.real_user_count().await;
        self.matrix
            .user_manager
            .max_local_users
            .fetch_max(current_count, std::sync::atomic::Ordering::Relaxed);
        self.matrix
            .user_manager
            .max_global_users
            .fetch_max(current_count, std::sync::atomic::Ordering::Relaxed);

        info!(nick = %nick, user = %user, uid = %self.uid, account = ?self.state.account, "Client registered");

        // 001 RPL_WELCOME
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
        self.write(welcome).await?;

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
        self.write(yourhost).await?;

        // 003 RPL_CREATED
        let created = server_reply(
            server_name,
            Response::RPL_CREATED,
            vec![
                nick.clone(),
                "This server was created at startup".to_string(),
            ],
        );
        self.write(created).await?;

        // 004 RPL_MYINFO
        let myinfo = server_reply(
            server_name,
            Response::RPL_MYINFO,
            vec![
                nick.clone(),
                server_name.to_string(),
                "slircd-ng-0.1.0".to_string(),
                "iowrZ".to_string(),
                "beIiklmnopqrstv".to_string(),
            ],
        );
        self.write(myinfo).await?;

        // Build ISUPPORT tokens using typed builders
        let chanmodes = ChanModesBuilder::new()
            .list_modes("beIq")
            .param_always("k")
            .param_set("l")
            .no_param("imnrstMU");

        let targmax = TargMaxBuilder::new()
            .add("JOIN", 10)
            .add("PART", 10)
            .add("KICK", 4)
            .add("PRIVMSG", 4)
            .add("NOTICE", 4)
            .add("NAMES", 10)
            .add("WHOIS", 1)
            .add("WHOWAS", 10);

        let builder = IsupportBuilder::new()
            .network(network)
            .casemapping("rfc1459")
            .chantypes("#&+!")
            .prefix("~&@%+", "qaohv")
            .chanmodes_typed(chanmodes)
            .max_nick_length(30)
            .custom("CHANNELLEN", Some("50"))
            .max_topic_length(390)
            .custom("KICKLEN", Some("390"))
            .custom("AWAYLEN", Some("200"))
            .modes_count(6)
            .custom("MAXTARGETS", Some("4"))
            .targmax(targmax)
            .custom("MONITOR", Some("100"))
            .excepts(Some('e'))
            .invex(Some('I'))
            .custom("EXTBAN", Some(",m"))
            .custom("ELIST", Some("MNU"))
            .status_msg("~&@%+")
            .custom("BOT", Some("B"))
            .custom("WHOX", None);

        // Send ISUPPORT lines (max 13 tokens per line to be safe)
        for line in builder.build_lines(13) {
            let reply = server_reply(
                server_name,
                Response::RPL_ISUPPORT,
                vec![
                    nick.clone(),
                    line,
                    "are supported by this server".to_string(),
                ],
            );
            self.write(reply).await?;
        }

        // 396 RPL_HOSTHIDDEN
        let hosthidden = server_reply(
            server_name,
            Response::RPL_HOSTHIDDEN,
            vec![
                nick.clone(),
                cloaked_host.clone(),
                "is now your displayed host".to_string(),
            ],
        );
        self.write(hosthidden).await?;

        // 375 RPL_MOTDSTART
        let motdstart = server_reply(
            server_name,
            Response::RPL_MOTDSTART,
            vec![
                nick.clone(),
                format!("- {} Message of the Day -", server_name),
            ],
        );
        self.write(motdstart).await?;

        // 372 RPL_MOTD - stream each line directly to transport
        for line in &self.matrix.server_info.motd_lines {
            let motd = server_reply(
                server_name,
                Response::RPL_MOTD,
                vec![nick.clone(), format!("- {}", line)],
            );
            self.write(motd).await?;
        }

        // 376 RPL_ENDOFMOTD
        let endmotd = server_reply(
            server_name,
            Response::RPL_ENDOFMOTD,
            vec![nick.clone(), "End of /MOTD command.".to_string()],
        );
        self.write(endmotd).await?;

        // Notify MONITOR watchers
        notify_monitors_online(self.matrix, nick, user, &cloaked_host).await;

        // Send snomask 'c' (Connect)
        self.matrix
            .user_manager
            .send_snomask(
                'c',
                &format!("Client connecting: {} ({}) [{}]", nick, user, ban_host),
            )
            .await;

        Ok(())
    }
}
