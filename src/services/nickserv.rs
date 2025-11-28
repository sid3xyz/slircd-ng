//! NickServ - Nickname registration and identification service.
//!
//! Handles:
//! - REGISTER <password> [email] - Register current nick
//! - IDENTIFY <password> - Identify to account
//! - GHOST <nick> - Kill session using your nick
//! - INFO <nick> - Show account information
//! - SET <option> <value> - Configure account settings

use crate::db::Database;
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// NickServ service.
pub struct NickServ {
    db: Database,
}

/// Result of a NickServ command.
pub struct NickServResult {
    /// Messages to send back to the user.
    pub replies: Vec<Message>,
    /// Account name if user is now identified.
    pub account: Option<String>,
    /// UID to kill (for GHOST command).
    pub kill_uid: Option<String>,
}

impl NickServ {
    /// Create a new NickServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Handle a PRIVMSG to NickServ.
    pub async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> NickServResult {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return self.help_reply();
        }

        let command = parts[0].to_uppercase();
        let args = &parts[1..];

        match command.as_str() {
            "REGISTER" => self.handle_register(nick, args).await,
            "IDENTIFY" => self.handle_identify(nick, args).await,
            "GHOST" => self.handle_ghost(matrix, uid, nick, args).await,
            "INFO" => self.handle_info(args).await,
            "SET" => self.handle_set(matrix, uid, args).await,
            "HELP" => self.help_reply(),
            _ => self.unknown_command(&command),
        }
    }

    /// Handle REGISTER command.
    async fn handle_register(&self, nick: &str, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: REGISTER <password> [email]");
        }

        let password = args[0];
        let email = args.get(1).copied();

        match self.db.accounts().register(nick, password, email).await {
            Ok(account) => {
                info!(nick = %nick, account = %account.name, "Account registered");
                NickServResult {
                    replies: vec![
                        self.notice_msg(&format!("Your nickname \x02{}\x02 has been registered.", nick)),
                        self.notice_msg("You are now identified to your account."),
                    ],
                    account: Some(account.name),
                    kill_uid: None,
                }
            }
            Err(crate::db::DbError::AccountExists(name)) => {
                self.error_reply(&format!("An account named \x02{}\x02 already exists.", name))
            }
            Err(crate::db::DbError::NicknameRegistered(name)) => {
                self.error_reply(&format!("The nickname \x02{}\x02 is already registered.", name))
            }
            Err(e) => {
                warn!(nick = %nick, error = ?e, "Registration failed");
                self.error_reply("Registration failed. Please try again later.")
            }
        }
    }

    /// Handle IDENTIFY command.
    async fn handle_identify(&self, nick: &str, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: IDENTIFY <password>");
        }

        let password = args[0];

        match self.db.accounts().identify(nick, password).await {
            Ok(account) => {
                info!(nick = %nick, account = %account.name, "User identified");
                NickServResult {
                    replies: vec![self.notice_msg(&format!(
                        "You are now identified for \x02{}\x02.",
                        account.name
                    ))],
                    account: Some(account.name),
                    kill_uid: None,
                }
            }
            Err(crate::db::DbError::AccountNotFound(_)) => {
                self.error_reply("No account found for your nickname.")
            }
            Err(crate::db::DbError::InvalidPassword) => {
                self.error_reply("Invalid password.")
            }
            Err(e) => {
                warn!(nick = %nick, error = ?e, "Identification failed");
                self.error_reply("Identification failed. Please try again later.")
            }
        }
    }

    /// Handle GHOST command.
    async fn handle_ghost(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> NickServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: GHOST <nick> [password]");
        }

        let target_nick = args[0];
        let password = args.get(1).copied();

        // Check if the user is already identified and get their account
        let user_account = if let Some(user) = matrix.users.get(uid) {
            let user = user.read().await;
            if user.modes.registered {
                user.account.clone()
            } else {
                None
            }
        } else {
            None
        };

        // Verify authorization
        let authorized = if let Some(ref account_name) = user_account {
            // User is identified, check if target nick belongs to their account
            if let Some(target_account) = self.db.accounts().find_by_nickname(target_nick).await.ok().flatten() {
                // Check if target belongs to the same account
                target_account.name.eq_ignore_ascii_case(account_name)
            } else {
                false
            }
        } else if let Some(pw) = password {
            // Try to identify with password
            self.db.accounts().identify(target_nick, pw).await.is_ok()
        } else {
            false
        };

        if !authorized {
            return self.error_reply("Access denied. You must be identified or provide the correct password.");
        }

        // Find the target user
        let target_nick_lower = slirc_proto::irc_to_lower(target_nick);
        if let Some(target_uid) = matrix.nicks.get(&target_nick_lower).map(|r| r.clone()) {
            if target_uid == uid {
                return self.error_reply("You cannot ghost yourself.");
            }

            info!(nick = %nick, target = %target_nick, "Ghost requested");
            NickServResult {
                replies: vec![self.notice_msg(&format!(
                    "\x02{}\x02 has been ghosted.",
                    target_nick
                ))],
                account: None,
                kill_uid: Some(target_uid),
            }
        } else {
            self.error_reply(&format!("\x02{}\x02 is not online.", target_nick))
        }
    }

    /// Handle INFO command.
    async fn handle_info(&self, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: INFO <nick>");
        }

        let nick = args[0];

        match self.db.accounts().find_by_nickname(nick).await {
            Ok(Some(account)) => {
                let registered_dt = chrono::DateTime::from_timestamp(account.registered_at, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                let last_seen_dt = chrono::DateTime::from_timestamp(account.last_seen_at, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                let mut replies = vec![
                    self.notice_msg(&format!("Information on \x02{}\x02:", account.name)),
                    self.notice_msg(&format!("  Registered: {}", registered_dt)),
                    self.notice_msg(&format!("  Last seen:  {}", last_seen_dt)),
                ];

                if !account.hide_email
                    && let Some(email) = &account.email
                {
                    replies.push(self.notice_msg(&format!("  Email:      {}", email)));
                }

                if account.enforce {
                    replies.push(self.notice_msg("  Options:    ENFORCE ON"));
                }

                // Get linked nicknames
                if let Ok(nicks) = self.db.accounts().get_nicknames(account.id).await
                    && !nicks.is_empty()
                {
                    replies.push(self.notice_msg(&format!(
                        "  Nicknames:  {}",
                        nicks.join(", ")
                    )));
                }

                NickServResult {
                    replies,
                    account: None,
                    kill_uid: None,
                }
            }
            Ok(None) => {
                self.error_reply(&format!("\x02{}\x02 is not registered.", nick))
            }
            Err(e) => {
                debug!(nick = %nick, error = ?e, "INFO lookup failed");
                self.error_reply("Failed to retrieve account information.")
            }
        }
    }

    /// Handle SET command.
    async fn handle_set(&self, matrix: &Arc<Matrix>, uid: &str, args: &[&str]) -> NickServResult {
        if args.len() < 2 {
            return NickServResult {
                replies: vec![
                    self.notice_msg("Syntax: SET <option> <value>"),
                    self.notice_msg("Options:"),
                    self.notice_msg("  EMAIL <address> - Set email address"),
                    self.notice_msg("  ENFORCE ON|OFF  - Enable/disable nickname enforcement"),
                    self.notice_msg("  HIDEMAIL ON|OFF - Hide/show email in INFO"),
                    self.notice_msg("  PASSWORD <pass> - Change password"),
                ],
                account: None,
                kill_uid: None,
            };
        }

        // Check if user is identified and get their account name
        let account_name = if let Some(user) = matrix.users.get(uid) {
            let user = user.read().await;
            if !user.modes.registered {
                return self.error_reply("You are not identified to any account.");
            }
            match &user.account {
                Some(name) => name.clone(),
                None => return self.error_reply("You are not identified to any account."),
            }
        } else {
            return self.error_reply("Internal error.");
        };

        // Find account
        let account = match self.db.accounts().find_by_name(&account_name).await {
            Ok(Some(acc)) => acc,
            _ => return self.error_reply("Account not found."),
        };

        let option = args[0];
        let value = args[1];

        match self.db.accounts().set_option(account.id, option, value).await {
            Ok(()) => {
                info!(account = %account.name, option = %option, "Account setting changed");
                NickServResult {
                    replies: vec![self.notice_msg(&format!(
                        "\x02{}\x02 has been set to \x02{}\x02.",
                        option.to_uppercase(),
                        value
                    ))],
                    account: None,
                    kill_uid: None,
                }
            }
            Err(e) => {
                warn!(account = %account.name, option = %option, error = ?e, "SET failed");
                self.error_reply("Failed to update setting.")
            }
        }
    }

    /// Create an error reply.
    fn error_reply(&self, msg: &str) -> NickServResult {
        NickServResult {
            replies: vec![self.notice_msg(msg)],
            account: None,
            kill_uid: None,
        }
    }

    /// Create a help reply.
    fn help_reply(&self) -> NickServResult {
        NickServResult {
            replies: vec![
                self.notice_msg("NickServ allows you to register and protect your nickname."),
                self.notice_msg("Commands:"),
                self.notice_msg("  \x02REGISTER\x02 <password> [email] - Register your nickname"),
                self.notice_msg("  \x02IDENTIFY\x02 <password>         - Identify to your account"),
                self.notice_msg("  \x02GHOST\x02 <nick> [password]     - Kill session using your nick"),
                self.notice_msg("  \x02INFO\x02 <nick>                 - Show account information"),
                self.notice_msg("  \x02SET\x02 <option> <value>        - Configure account settings"),
                self.notice_msg("  \x02HELP\x02                        - Show this help"),
            ],
            account: None,
            kill_uid: None,
        }
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, cmd: &str) -> NickServResult {
        NickServResult {
            replies: vec![self.notice_msg(&format!(
                "Unknown command \x02{}\x02. Type \x02HELP\x02 for a list of commands.",
                cmd
            ))],
            account: None,
            kill_uid: None,
        }
    }

    /// Create a NOTICE message from NickServ.
    /// Note: The target is set to an empty string and will be filled in by
    /// the route_service_message function with the actual recipient nick.
    fn notice_msg(&self, text: &str) -> Message {
        Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                "NickServ".to_string(),
                "NickServ".to_string(),
                "services.".to_string(),
            )),
            // Target is empty and will be replaced by route_service_message
            command: Command::NOTICE(String::new(), text.to_string()),
        }
    }
}

/// Handle service message routing.
pub async fn route_service_message(
    matrix: &Arc<Matrix>,
    db: &Database,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &mpsc::Sender<Message>,
) -> bool {
    let target_lower = target.to_lowercase();

    if target_lower == "nickserv" || target_lower == "ns" {
        let nickserv = NickServ::new(db.clone());
        let result = nickserv.handle(matrix, uid, nick, text).await;

        // Send replies
        for mut reply in result.replies {
            // Set the target for the NOTICE
            if let Command::NOTICE(_, text) = &reply.command {
                reply.command = Command::NOTICE(nick.to_string(), text.clone());
            }
            let _ = sender.send(reply).await;
        }

        // Handle account identification
        if let Some(account_name) = result.account {
            // Set +r mode and account on user
            if let Some(user) = matrix.users.get(uid) {
                let mut user = user.write().await;
                user.modes.registered = true;
                user.account = Some(account_name.clone());
                info!(uid = %uid, account = %account_name, "User identified to account");
            }
        }

        // Handle GHOST kill
        if let Some(target_uid) = result.kill_uid {
            // Send KILL message to target to disconnect them
            if let Some(target_sender) = matrix.senders.get(&target_uid) {
                let quit_msg = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName("services.".to_string())),
                    command: Command::KILL(
                        target_uid.clone(),
                        "Ghosted by NickServ".to_string(),
                    ),
                };
                let _ = target_sender.send(quit_msg).await;
            }
        }

        true
    } else {
        false
    }
}
