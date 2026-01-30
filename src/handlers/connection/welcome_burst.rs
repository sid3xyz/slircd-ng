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
use crate::handlers::SaslState;
use crate::handlers::{apply_user_modes_typed, notify_monitors_online, server_reply};
use crate::state::{Matrix, UnregisteredState, User};
use slirc_proto::isupport::{ChanModesBuilder, IsupportBuilder, TargMaxBuilder};
use slirc_proto::mode::{Mode, ModeType, UserMode};
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

    /// Validate potential nick collisions, handling multiclient logic if enabled.
    async fn validate_multiclient_collision(&mut self, nick: &str) -> Result<(), HandlerError> {
        if !self.matrix.config.multiclient.enabled {
            return Ok(());
        }

        let server_name = &self.matrix.server_info.name;

        // Per-account override: if multiclient is disabled for this account, reject additional sessions
        if let Some(ref account_name) = self.state.account {
            let override_opt = self
                .matrix
                .client_manager
                .get_multiclient_override(account_name);
            if !self
                .matrix
                .config
                .multiclient
                .is_multiclient_enabled(override_opt)
            {
                let nick_lower = slirc_proto::irc_to_lower(nick);
                if let Some(existing) = self.matrix.user_manager.nicks.get(&nick_lower) {
                    let has_other = existing.value().iter().any(|uid| uid != self.uid);
                    if has_other {
                        // Nick is already in use by an existing session; reject this connection
                        let reply = Response::err_nicknameinuse(nick, nick)
                            .with_prefix(Prefix::ServerName(server_name.to_string()));
                        self.write(reply).await?;
                        let error = Message::from(Command::ERROR(
                            "Closing Link: Nickname in use (multiclient disabled)".to_string(),
                        ));
                        self.write(error).await?;
                        return Err(HandlerError::NicknameInUse(nick.to_string()));
                    }
                }
            }
        }

        // Reject duplicate nick when no account is present (no bouncer)
        if self.state.account.is_none() {
            let nick_lower = slirc_proto::irc_to_lower(nick);
            if let Some(existing) = self.matrix.user_manager.nicks.get(&nick_lower) {
                let has_other = existing.value().iter().any(|uid| uid != self.uid);
                if has_other {
                    let reply = Response::err_nicknameinuse(nick, nick)
                        .with_prefix(Prefix::ServerName(server_name.to_string()));
                    self.write(reply).await?;
                    let error =
                        Message::from(Command::ERROR("Closing Link: Nickname in use".to_string()));
                    self.write(error).await?;
                    return Err(HandlerError::NicknameInUse(nick.to_string()));
                }
            }
        }

        let nick_lower = slirc_proto::irc_to_lower(nick);
        if let Some(existing_uids) = self.matrix.user_manager.nicks.get(&nick_lower) {
            let existing_uids_vec = existing_uids.value().clone();
            // If more than one UID, validate they all share the same account
            if existing_uids_vec.len() > 1 {
                let current_account = self.state.account.clone();

                tracing::debug!(
                    nick = %nick,
                    uid = %self.uid,
                    current_account = ?current_account,
                    existing_uids = ?existing_uids_vec,
                    "Validating nick collision for multiclient"
                );

                // Get account of first existing UID (should be registered by now)
                let existing_account = if let Some(first_uid) = existing_uids_vec.first() {
                    if first_uid != self.uid {
                        if let Some(user_arc) =
                            self.matrix.user_manager.users.get(first_uid.as_str())
                        {
                            let user = user_arc.read().await;
                            user.account.clone()
                        } else {
                            // OTHER UID IS NOT REGISTERED (No User object)
                            // This means we are racing another pre-reg connection.
                            // We ignore it and proceed - let "First to Register" logic win.
                            tracing::info!(
                                nick = %nick,
                                uid = %self.uid,
                                other_uid = %first_uid,
                                "Ignoring collision with unregistered UID (race condition resolution)"
                            );
                            return Ok(());
                        }
                    } else {
                        // If first UID is self, check second UID
                        if let Some(second_uid) = existing_uids_vec.get(1) {
                            if let Some(user_arc) =
                                self.matrix.user_manager.users.get(second_uid.as_str())
                            {
                                let user = user_arc.read().await;
                                user.account.clone()
                            } else {
                                // OTHER UID IS NOT REGISTERED
                                tracing::info!(
                                    nick = %nick,
                                    uid = %self.uid,
                                    other_uid = %second_uid,
                                    "Ignoring collision with unregistered UID (race condition resolution)"
                                );
                                return Ok(());
                            }
                        } else {
                            None
                        }
                    }
                } else {
                    None
                };

                tracing::debug!(
                    nick = %nick,
                    uid = %self.uid,
                    existing_account = ?existing_account,
                    "Compared accounts for validation"
                );

                // Reject if accounts don't match
                match (current_account, existing_account) {
                    (Some(ref cur_acc), Some(ref exist_acc)) if cur_acc == exist_acc => {
                        // Valid multiclient - accounts match
                        tracing::info!(
                            nick = %nick,
                            uid = %self.uid,
                            account = %cur_acc,
                            "Valid multiclient connection - accounts match"
                        );
                    }
                    _ => {
                        // Accounts don't match - remove this UID from the nick list and reject
                        tracing::warn!(
                            nick = %nick,
                            uid = %self.uid,
                            "Nick collision - accounts don't match, rejecting connection"
                        );

                        if let Some(mut entry) = self.matrix.user_manager.nicks.get_mut(&nick_lower)
                        {
                            entry.value_mut().retain(|uid| uid != self.uid);
                            if entry.value().is_empty() {
                                drop(entry);
                                self.matrix.user_manager.nicks.remove(&nick_lower);
                            }
                        }

                        let reply = Response::err_nicknameinuse(nick, nick)
                            .with_prefix(Prefix::ServerName(server_name.to_string()));
                        self.write(reply).await?;
                        let error = Message::from(Command::ERROR(
                            "Closing Link: Nickname collision (accounts don't match)".to_string(),
                        ));
                        self.write(error).await?;
                        return Err(HandlerError::NicknameInUse(nick.to_string()));
                    }
                }
            }
        }

        Ok(())
    }

    /// Send the complete welcome burst.
    ///
    /// Returns `Ok(true)` if this was a bouncer reattachment (shared existing User),
    /// `Ok(false)` if this was a normal registration (new User created).
    /// Returns an error if the client should be disconnected (e.g., banned, wrong password).
    #[allow(clippy::collapsible_if)]
    pub async fn send(mut self) -> Result<bool, HandlerError> {
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

        // Check if SASL authentication is required
        if self.matrix.config.security.require_sasl
            && self.state.sasl_state != SaslState::Authenticated
        {
            let reply = server_reply(
                server_name,
                Response::ERR_SASLFAIL,
                vec![nick.clone(), "SASL authentication is required".to_string()],
            );
            self.write(reply).await?;
            let error = Message::from(Command::ERROR(
                "Closing Link: Access denied (SASL authentication required)".to_string(),
            ));
            self.write(error).await?;
            return Err(HandlerError::AccessDenied);
        }

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

        // Validate nick collision for multiclient/bouncer mode
        self.validate_multiclient_collision(nick).await?;

        // Check if this is a bouncer reattachment (existing UID to share)
        // If so, we DON'T create a new User - we reuse the existing one
        if let Some(ref reattach_info) = self.state.reattach_info
            && let Some(ref existing_uid) = reattach_info.existing_uid
            && let Some(user_arc) = self.matrix.user_manager.users.get(existing_uid)
        {
            // Bouncer reattachment: this connection shares an existing User
            // Look up the existing user to get their info for the welcome burst
            let existing_user = user_arc.read().await;
            let cloaked_host = existing_user.visible_host.clone();
            let existing_nick = existing_user.nick.clone();
            let existing_user_name = existing_user.user.clone();

            tracing::info!(
                new_session_id = %self.state.session_id,
                existing_uid = %existing_uid,
                nick = %existing_nick,
                account = %reattach_info.account,
                "Bouncer reattachment: sharing existing User"
            );

            // Decrement unregistered counter (connection is now "registered" by sharing)
            self.matrix.user_manager.decrement_unregistered();

            // Send welcome burst using existing user info
            // 001 RPL_WELCOME
            let welcome = server_reply(
                server_name,
                Response::RPL_WELCOME,
                vec![
                    existing_nick.clone(),
                    format!(
                        "Welcome to the {} IRC Network {}!{}@{}",
                        network, existing_nick, existing_user_name, cloaked_host
                    ),
                ],
            );
            self.write(welcome).await?;

            // 002 RPL_YOURHOST
            let yourhost = server_reply(
                server_name,
                Response::RPL_YOURHOST,
                vec![
                    existing_nick.clone(),
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
                    existing_nick.clone(),
                    "This server was created at startup".to_string(),
                ],
            );
            self.write(created).await?;

            // 004 RPL_MYINFO
            let myinfo = server_reply(
                server_name,
                Response::RPL_MYINFO,
                vec![
                    existing_nick.clone(),
                    server_name.to_string(),
                    "slircd-ng-0.1.0".to_string(),
                    "iowrZ".to_string(),
                    "beIiklmnopqrstv".to_string(),
                ],
            );
            self.write(myinfo).await?;

            // 005 RPL_ISUPPORT - use the same builder as normal registration
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
                .custom("METADATA", None)
                .casemapping(self.matrix.config.server.casemapping.as_isupport_value())
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
                .custom("WHOX", None)
                .custom("UTF8ONLY", None);

            for line in builder.build_lines(13) {
                let reply = server_reply(
                    server_name,
                    Response::RPL_ISUPPORT,
                    vec![
                        existing_nick.clone(),
                        line,
                        "are supported by this server".to_string(),
                    ],
                );
                self.write(reply).await?;
            }

            // 396 RPL_HOSTHIDDEN
            let hostmask = server_reply(
                server_name,
                Response::RPL_HOSTHIDDEN,
                vec![
                    existing_nick.clone(),
                    cloaked_host.clone(),
                    "is now your displayed host".to_string(),
                ],
            );
            self.write(hostmask).await?;

            // 375 RPL_MOTDSTART
            let motdstart = server_reply(
                server_name,
                Response::RPL_MOTDSTART,
                vec![
                    existing_nick.clone(),
                    format!("- {} Message of the Day -", server_name),
                ],
            );
            self.write(motdstart).await?;

            // 372 RPL_MOTD
            for line in &self.matrix.server_info.motd_lines {
                let motd = server_reply(
                    server_name,
                    Response::RPL_MOTD,
                    vec![existing_nick.clone(), format!("- {}", line)],
                );
                self.write(motd).await?;
            }

            // 376 RPL_ENDOFMOTD
            let endmotd = server_reply(
                server_name,
                Response::RPL_ENDOFMOTD,
                vec![existing_nick.clone(), "End of /MOTD command.".to_string()],
            );
            self.write(endmotd).await?;

            // Auto-join existing channels (replay channel state)
            for (channel_name, membership) in &reattach_info.channels {
                // Send synthetic JOIN to the client
                let join_msg = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
                        existing_nick.clone(),
                        existing_user_name.clone(),
                        cloaked_host.clone(),
                    )),
                    command: Command::JOIN(channel_name.clone(), None, None),
                };
                self.write(join_msg).await?;

                tracing::debug!(
                    channel = %channel_name,
                    modes = %membership.modes,
                    "Replaying channel join for reattached session"
                );
            }

            // Fix UID Leak (Innovation 1: Operational Safety)
            // Remove the temporary session UID from the nicks map.
            // We are adopting the existing user's identity (living at existing_uid),
            // so this transient UID should not be associated with the nickname anymore.
            let input_nick_lower = slirc_proto::irc_to_lower(nick);
            if let Some(mut entry) = self.matrix.user_manager.nicks.get_mut(&input_nick_lower) {
                entry.retain(|u| u != self.uid);
                // Note: The map entry should not be empty because existing_uid should be there
                // (or will be added if not, though it should be there because existing_user exists).
                // If it becomes empty (edge case), we should remove it, but dashmap entry API makes that tricky safely here.
                // Given existing_user exists, it's fine.
            }

            return Ok(true); // Reattachment successful
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
            session_id: self.state.session_id,
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

        // Apply default user modes from config (e.g., "+i" for default invisible)
        if let Some(ref default_modes) = self.matrix.config.server.default_user_modes {
            let modes = parse_default_user_modes(default_modes);
            if !modes.is_empty() {
                apply_user_modes_typed(&mut user_obj.modes, &modes);
            }
        }

        let cloaked_host = user_obj.visible_host.clone();

        let is_starting_invisible = user_obj.modes.invisible;
        let is_starting_oper = user_obj.modes.oper;

        self.matrix.user_manager.add_local_user(user_obj).await;

        // User is now registered - decrement unregistered connection count
        self.matrix.user_manager.decrement_unregistered();

        crate::metrics::inc_connected_users();

        // Update StatsManager counters
        // Note: user_connected() is now called inside add_local_user()

        if is_starting_invisible {
            self.matrix.stats_manager.user_set_invisible();
        }
        if is_starting_oper {
            self.matrix.stats_manager.user_opered();
        }

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
            .param_set("fl") // l = limit, f = flood protection (takes param on set)
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
            .custom("METADATA", None) // Early in the list to pass buggy tests
            .casemapping(self.matrix.config.server.casemapping.as_isupport_value())
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
            .custom(
                "CHATHISTORY",
                Some(&crate::handlers::chathistory::MAX_HISTORY_LIMIT_CONST.to_string()),
            )
            .custom("MSGREFTYPES", Some("timestamp,msgid"))
            .custom("EXTBAN", Some(",m"))
            .custom("ELIST", Some("MNU"))
            .status_msg("~&@%+")
            .custom("BOT", Some("B"))
            .custom("WHOX", None)
            .custom("UTF8ONLY", None); // Advertise UTF-8 only mode per modern IRC

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

        Ok(false) // Normal registration (new User created)
    }
}

// Helper to parse default user modes from config string
// e.g. "+iw" -> [Invisible, Wallops]
// Helper to parse default user modes from config string
// e.g. "+iw" -> [Add(Invisible), Add(Wallops)]
fn parse_default_user_modes(mode_str: &str) -> Vec<Mode<UserMode>> {
    let mut modes = Vec::new();
    let mut chars = mode_str.chars();

    // Skip leading '+' if present
    if mode_str.starts_with('+') {
        chars.next();
    }

    for c in chars {
        // Compiler suggested Mode::plus(mode, None) constructor
        let mode = UserMode::from_char(c);
        modes.push(Mode::plus(mode, None));
    }
    modes
}
