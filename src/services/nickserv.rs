//! NickServ - Nickname registration and identification service.
//!
//! Handles:
//! - REGISTER <password> [email] - Register current nick
//! - IDENTIFY <password> - Identify to account
//! - GHOST <nick> - Kill session using your nick
//! - INFO <nick> - Show account information
//! - SET <option> <value> - Configure account settings

use crate::db::Database;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use slirc_proto::{irc_to_lower, Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// NickServ service.
pub struct NickServ {
    db: Database,
}

/// Result of a NickServ command - a list of effects to apply.
pub type NickServResult = Vec<ServiceEffect>;

impl NickServ {
    /// Create a new NickServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Handle a PRIVMSG to NickServ.
    /// Returns a list of effects that the caller should apply.
    pub async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> NickServResult {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return self.help_reply(uid);
        }

        let command = parts[0].to_uppercase();
        let args = &parts[1..];

        match command.as_str() {
            "REGISTER" => self.handle_register(uid, nick, args).await,
            "IDENTIFY" => self.handle_identify(uid, nick, args).await,
            "DROP" => self.handle_drop(uid, nick, args).await,
            "GROUP" => self.handle_group(uid, nick, args).await,
            "UNGROUP" => self.handle_ungroup(matrix, uid, args).await,
            "GHOST" => self.handle_ghost(matrix, uid, nick, args).await,
            "INFO" => self.handle_info(uid, args).await,
            "SET" => self.handle_set(matrix, uid, args).await,
            "HELP" => self.help_reply(uid),
            _ => self.unknown_command(uid, &command),
        }
    }

    /// Handle REGISTER command.
    async fn handle_register(&self, uid: &str, nick: &str, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.reply_effects(uid, vec!["Syntax: REGISTER <password> [email]"]);
        }

        let password = args[0];
        let email = args.get(1).copied();

        match self.db.accounts().register(nick, password, email).await {
            Ok(account) => {
                info!(nick = %nick, account = %account.name, "Account registered");
                vec![
                    self.reply_effect(uid, &format!("Your nickname \x02{}\x02 has been registered.", nick)),
                    self.reply_effect(uid, "You are now identified to your account."),
                    ServiceEffect::AccountIdentify {
                        target_uid: uid.to_string(),
                        account: account.name,
                    },
                ]
            }
            Err(crate::db::DbError::AccountExists(name)) => {
                self.reply_effects(uid, vec![&format!("An account named \x02{}\x02 already exists.", name)])
            }
            Err(crate::db::DbError::NicknameRegistered(name)) => {
                self.reply_effects(uid, vec![&format!("The nickname \x02{}\x02 is already registered.", name)])
            }
            Err(e) => {
                warn!(nick = %nick, error = ?e, "Registration failed");
                self.reply_effects(uid, vec!["Registration failed. Please try again later."])
            }
        }
    }

    /// Handle IDENTIFY command.
    async fn handle_identify(&self, uid: &str, nick: &str, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.reply_effects(uid, vec!["Syntax: IDENTIFY <password>"]);
        }

        let password = args[0];

        match self.db.accounts().identify(nick, password).await {
            Ok(account) => {
                info!(nick = %nick, account = %account.name, "User identified");
                vec![
                    self.reply_effect(uid, &format!("You are now identified for \x02{}\x02.", account.name)),
                    ServiceEffect::AccountIdentify {
                        target_uid: uid.to_string(),
                        account: account.name,
                    },
                ]
            }
            Err(crate::db::DbError::AccountNotFound(_)) => {
                self.reply_effects(uid, vec!["No account found for your nickname."])
            }
            Err(crate::db::DbError::InvalidPassword) => {
                self.reply_effects(uid, vec!["Invalid password."])
            }
            Err(e) => {
                warn!(nick = %nick, error = ?e, "Identification failed");
                self.reply_effects(uid, vec!["Identification failed. Please try again later."])
            }
        }
    }

    /// Handle DROP command.
    async fn handle_drop(
        &self,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> NickServResult {
        if args.is_empty() {
            return self.reply_effects(uid, vec!["Syntax: DROP <password>"]);
        }

        let password = args[0];

        // Verify the user owns the account for their current nick
        match self.db.accounts().drop_account(nick, password).await {
            Ok(()) => {
                info!(nick = %nick, "Account dropped");
                vec![
                    self.reply_effect(uid, &format!("Your account \x02{}\x02 has been dropped.", nick)),
                    self.reply_effect(uid, "All associated nicknames have been released."),
                    ServiceEffect::AccountClear {
                        target_uid: uid.to_string(),
                    },
                ]
            }
            Err(crate::db::DbError::AccountNotFound(_)) => {
                self.reply_effects(uid, vec!["Your nickname is not registered."])
            }
            Err(crate::db::DbError::InvalidPassword) => {
                self.reply_effects(uid, vec!["Invalid password."])
            }
            Err(e) => {
                warn!(nick = %nick, error = ?e, "DROP failed");
                self.reply_effects(uid, vec!["Failed to drop account. Please try again later."])
            }
        }
    }

    /// Handle GROUP command - link current nick to an existing account.
    async fn handle_group(
        &self,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> NickServResult {
        if args.len() < 2 {
            return self.reply_effects(uid, vec!["Syntax: GROUP <account> <password>"]);
        }

        let account_name = args[0];
        let password = args[1];

        match self.db.accounts().link_nickname(nick, account_name, password).await {
            Ok(()) => {
                info!(nick = %nick, account = %account_name, "Nickname grouped");
                vec![
                    self.reply_effect(uid, &format!(
                        "Your nickname \x02{}\x02 is now linked to account \x02{}\x02.",
                        nick, account_name
                    )),
                    self.reply_effect(uid, "You are now identified to your account."),
                    ServiceEffect::AccountIdentify {
                        target_uid: uid.to_string(),
                        account: account_name.to_string(),
                    },
                ]
            }
            Err(crate::db::DbError::AccountNotFound(_)) => {
                self.reply_effects(uid, vec![&format!("Account \x02{}\x02 does not exist.", account_name)])
            }
            Err(crate::db::DbError::InvalidPassword) => {
                self.reply_effects(uid, vec!["Invalid password."])
            }
            Err(crate::db::DbError::NicknameRegistered(_)) => {
                self.reply_effects(uid, vec![&format!(
                    "Nickname \x02{}\x02 is already registered to another account.",
                    nick
                )])
            }
            Err(e) => {
                warn!(nick = %nick, account = %account_name, error = ?e, "GROUP failed");
                self.reply_effects(uid, vec!["Failed to group nickname. Please try again later."])
            }
        }
    }

    /// Handle UNGROUP command - unlink a nick from the current account.
    async fn handle_ungroup(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        args: &[&str],
    ) -> NickServResult {
        if args.is_empty() {
            return self.reply_effects(uid, vec!["Syntax: UNGROUP <nick>"]);
        }

        let target_nick = args[0];

        // Must be identified first
        let (account_name, account_id) = if let Some(user) = matrix.users.get(uid) {
            let user = user.read().await;
            if !user.modes.registered {
                return self.reply_effects(uid, vec!["You must be identified to use this command."]);
            }
            match &user.account {
                Some(name) => {
                    // Look up the account ID
                    match self.db.accounts().find_by_name(name).await {
                        Ok(Some(acc)) => (name.clone(), acc.id),
                        _ => return self.reply_effects(uid, vec!["Account not found."]),
                    }
                }
                None => return self.reply_effects(uid, vec!["You are not identified to any account."]),
            }
        } else {
            return self.reply_effects(uid, vec!["Internal error."]);
        };

        match self.db.accounts().unlink_nickname(target_nick, account_id).await {
            Ok(()) => {
                info!(nick = %target_nick, account = %account_name, "Nickname ungrouped");
                self.reply_effects(uid, vec![&format!(
                    "Nickname \x02{}\x02 has been removed from your account.",
                    target_nick
                )])
            }
            Err(crate::db::DbError::NicknameNotFound(_)) => {
                self.reply_effects(uid, vec![&format!(
                    "Nickname \x02{}\x02 is not linked to your account.",
                    target_nick
                )])
            }
            Err(crate::db::DbError::InsufficientAccess) => {
                self.reply_effects(uid, vec![&format!(
                    "Nickname \x02{}\x02 does not belong to your account.",
                    target_nick
                )])
            }
            Err(crate::db::DbError::UnknownOption(msg)) => {
                self.reply_effects(uid, vec![&msg])
            }
            Err(e) => {
                warn!(nick = %target_nick, error = ?e, "UNGROUP failed");
                self.reply_effects(uid, vec!["Failed to ungroup nickname. Please try again later."])
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
            return self.reply_effects(uid, vec!["Syntax: GHOST <nick> [password]"]);
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
            return self.reply_effects(uid, vec!["Access denied. You must be identified or provide the correct password."]);
        }

        // Find the target user
        let target_nick_lower = slirc_proto::irc_to_lower(target_nick);
        if let Some(target_uid) = matrix.nicks.get(&target_nick_lower).map(|r| r.clone()) {
            if target_uid == uid {
                return self.reply_effects(uid, vec!["You cannot ghost yourself."]);
            }

            info!(nick = %nick, target = %target_nick, "Ghost requested");
            vec![
                self.reply_effect(uid, &format!("\x02{}\x02 has been ghosted.", target_nick)),
                ServiceEffect::Kill {
                    target_uid,
                    killer: "NickServ".to_string(),
                    reason: format!("Ghosted by {}", nick),
                },
            ]
        } else {
            self.reply_effects(uid, vec![&format!("\x02{}\x02 is not online.", target_nick)])
        }
    }

    /// Handle INFO command.
    async fn handle_info(&self, uid: &str, args: &[&str]) -> NickServResult {
        if args.is_empty() {
            return self.reply_effects(uid, vec!["Syntax: INFO <nick>"]);
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

                let mut effects = vec![
                    self.reply_effect(uid, &format!("Information on \x02{}\x02:", account.name)),
                    self.reply_effect(uid, &format!("  Registered: {}", registered_dt)),
                    self.reply_effect(uid, &format!("  Last seen:  {}", last_seen_dt)),
                ];

                if !account.hide_email
                    && let Some(email) = &account.email
                {
                    effects.push(self.reply_effect(uid, &format!("  Email:      {}", email)));
                }

                if account.enforce {
                    effects.push(self.reply_effect(uid, "  Options:    ENFORCE ON"));
                }

                // Get linked nicknames
                if let Ok(nicks) = self.db.accounts().get_nicknames(account.id).await
                    && !nicks.is_empty()
                {
                    effects.push(self.reply_effect(uid, &format!(
                        "  Nicknames:  {}",
                        nicks.join(", ")
                    )));
                }

                effects
            }
            Ok(None) => {
                self.reply_effects(uid, vec![&format!("\x02{}\x02 is not registered.", nick)])
            }
            Err(e) => {
                debug!(nick = %nick, error = ?e, "INFO lookup failed");
                self.reply_effects(uid, vec!["Failed to retrieve account information."])
            }
        }
    }

    /// Handle SET command.
    async fn handle_set(&self, matrix: &Arc<Matrix>, uid: &str, args: &[&str]) -> NickServResult {
        if args.len() < 2 {
            return vec![
                self.reply_effect(uid, "Syntax: SET <option> <value>"),
                self.reply_effect(uid, "Options:"),
                self.reply_effect(uid, "  EMAIL <address> - Set email address"),
                self.reply_effect(uid, "  ENFORCE ON|OFF  - Enable/disable nickname enforcement"),
                self.reply_effect(uid, "  HIDEMAIL ON|OFF - Hide/show email in INFO"),
                self.reply_effect(uid, "  PASSWORD <pass> - Change password"),
            ];
        }

        // Check if user is identified and get their account name
        let account_name = if let Some(user) = matrix.users.get(uid) {
            let user = user.read().await;
            if !user.modes.registered {
                return self.reply_effects(uid, vec!["You are not identified to any account."]);
            }
            match &user.account {
                Some(name) => name.clone(),
                None => return self.reply_effects(uid, vec!["You are not identified to any account."]),
            }
        } else {
            return self.reply_effects(uid, vec!["Internal error."]);
        };

        // Find account
        let account = match self.db.accounts().find_by_name(&account_name).await {
            Ok(Some(acc)) => acc,
            _ => return self.reply_effects(uid, vec!["Account not found."]),
        };

        let option = args[0];
        let value = args[1];

        match self.db.accounts().set_option(account.id, option, value).await {
            Ok(()) => {
                info!(account = %account.name, option = %option, "Account setting changed");
                self.reply_effects(uid, vec![&format!(
                    "\x02{}\x02 has been set to \x02{}\x02.",
                    option.to_uppercase(),
                    value
                )])
            }
            Err(crate::db::DbError::UnknownOption(opt)) => {
                self.reply_effects(uid, vec![&format!("Unknown option: \x02{}\x02. Valid options: EMAIL, ENFORCE, HIDEMAIL, PASSWORD", opt)])
            }
            Err(e) => {
                warn!(account = %account.name, option = %option, error = ?e, "SET failed");
                self.reply_effects(uid, vec!["Failed to update setting."])
            }
        }
    }

    /// Create a single reply effect.
    fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: target_uid.to_string(),
            msg: Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    "NickServ".to_string(),
                    "NickServ".to_string(),
                    "services.".to_string(),
                )),
                command: Command::NOTICE(String::new(), text.to_string()),
            },
        }
    }

    /// Create multiple reply effects.
    fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> NickServResult {
        texts.into_iter()
            .map(|t| self.reply_effect(target_uid, t))
            .collect()
    }

    /// Create a help reply.
    fn help_reply(&self, uid: &str) -> NickServResult {
        vec![
            self.reply_effect(uid, "NickServ allows you to register and protect your nickname."),
            self.reply_effect(uid, "Commands:"),
            self.reply_effect(uid, "  \x02REGISTER\x02 <password> [email] - Register your nickname"),
            self.reply_effect(uid, "  \x02IDENTIFY\x02 <password>         - Identify to your account"),
            self.reply_effect(uid, "  \x02DROP\x02 <password>             - Delete your account"),
            self.reply_effect(uid, "  \x02GROUP\x02 <account> <password>  - Link nick to account"),
            self.reply_effect(uid, "  \x02UNGROUP\x02 <nick>              - Remove nick from account"),
            self.reply_effect(uid, "  \x02GHOST\x02 <nick> [password]     - Kill session using your nick"),
            self.reply_effect(uid, "  \x02INFO\x02 <nick>                 - Show account information"),
            self.reply_effect(uid, "  \x02SET\x02 <option> <value>        - Configure account settings"),
            self.reply_effect(uid, "  \x02HELP\x02                        - Show this help"),
        ]
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, uid: &str, cmd: &str) -> NickServResult {
        self.reply_effects(uid, vec![&format!(
            "Unknown command \x02{}\x02. Type \x02HELP\x02 for a list of commands.",
            cmd
        )])
    }
}

/// Handle service message routing.
/// Applies all effects returned by NickServ.
pub async fn route_service_message(
    matrix: &Arc<Matrix>,
    db: &Database,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &mpsc::Sender<Message>,
) -> bool {
    let target_lower = irc_to_lower(target);

    if target_lower == "nickserv" || target_lower == "ns" {
        let nickserv = NickServ::new(db.clone());
        let effects = nickserv.handle(matrix, uid, nick, text).await;

        // Apply each effect
        for effect in effects {
            apply_effect(matrix, nick, sender, effect).await;
        }

        true
    } else {
        false
    }
}

/// Apply a single service effect.
pub async fn apply_effect(
    matrix: &Arc<Matrix>,
    nick: &str,
    sender: &mpsc::Sender<Message>,
    effect: ServiceEffect,
) {
    match effect {
        ServiceEffect::Reply { target_uid: _, mut msg } => {
            // Set the target nick for the NOTICE
            if let Command::NOTICE(_, text) = &msg.command {
                msg.command = Command::NOTICE(nick.to_string(), text.clone());
            }
            let _ = sender.send(msg).await;
        }

        ServiceEffect::AccountIdentify { target_uid, account } => {
            // Get user info for MODE broadcast before we modify the user
            let (nick, user_str, host, channels) = {
                if let Some(user_ref) = matrix.users.get(&target_uid) {
                    let user = user_ref.read().await;
                    (
                        user.nick.clone(),
                        user.user.clone(),
                        user.host.clone(),
                        user.channels.iter().cloned().collect::<Vec<_>>(),
                    )
                } else {
                    return;
                }
            };

            // Set +r mode and account on user
            if let Some(user_ref) = matrix.users.get(&target_uid) {
                let mut user = user_ref.write().await;
                user.modes.registered = true;
                user.account = Some(account.clone());
            }

            // Clear any nick enforcement timer
            matrix.enforce_timers.remove(&target_uid);

            // Broadcast MODE +r to all channels the user is in
            let mode_msg = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                command: Command::UserMODE(
                    nick.clone(),
                    vec![slirc_proto::Mode::Plus(slirc_proto::UserMode::Registered, None)],
                ),
            };

            // Broadcast ACCOUNT message for account-notify capability (IRCv3.1)
            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT(account.clone()),
            };

            for channel_name in &channels {
                matrix.broadcast_to_channel(channel_name, mode_msg.clone(), None).await;
                matrix.broadcast_to_channel(channel_name, account_msg.clone(), None).await;
            }

            // Also send MODE and ACCOUNT to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(mode_msg).await;
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, nick = %nick, account = %account, "User identified to account");
        }

        ServiceEffect::AccountClear { target_uid } => {
            // Get user info for ACCOUNT broadcast before we modify the user
            let (nick, user_str, host, channels) = {
                if let Some(user_ref) = matrix.users.get(&target_uid) {
                    let user = user_ref.read().await;
                    (
                        user.nick.clone(),
                        user.user.clone(),
                        user.host.clone(),
                        user.channels.iter().cloned().collect::<Vec<_>>(),
                    )
                } else {
                    return;
                }
            };

            // Clear +r mode and account
            if let Some(user) = matrix.users.get(&target_uid) {
                let mut user = user.write().await;
                user.modes.registered = false;
                user.account = None;
            }

            // Broadcast ACCOUNT * for logout (account-notify capability)
            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT("*".to_string()),
            };

            for channel_name in &channels {
                matrix.broadcast_to_channel(channel_name, account_msg.clone(), None).await;
            }

            // Also send ACCOUNT * to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, "User account cleared");
        }

        ServiceEffect::ClearEnforceTimer { target_uid } => {
            matrix.enforce_timers.remove(&target_uid);
        }

        ServiceEffect::Kill { target_uid, killer, reason } => {
            // Use centralized disconnect logic
            let quit_reason = format!("Killed by {} ({})", killer, reason);
            matrix.disconnect_user(&target_uid, &quit_reason).await;
            info!(target = %target_uid, killer = %killer, reason = %reason, "User killed by service");
        }

        ServiceEffect::ChannelMode { channel, target_uid, mode_char, adding } => {
            // Get target nick for MODE message
            let target_nick = if let Some(user_ref) = matrix.users.get(&target_uid) {
                user_ref.read().await.nick.clone()
            } else {
                return;
            };

            // Get canonical channel name
            let canonical_name = if let Some(channel_ref) = matrix.channels.get(&channel) {
                channel_ref.read().await.name.clone()
            } else {
                return;
            };

            // Apply channel mode change
            if let Some(channel_ref) = matrix.channels.get(&channel) {
                let mut channel_guard = channel_ref.write().await;
                if let Some(member) = channel_guard.members.get_mut(&target_uid) {
                    match mode_char {
                        'o' => member.op = adding,
                        'v' => member.voice = adding,
                        _ => {}
                    }
                }
            }

            // Build MODE message from ChanServ
            // NOTE: Using Command::Raw for dynamic single-mode change. The mode_char
            // comes from service effect, not parsed from wire, so we build manually.
            let mode_str = if adding {
                format!("+{}", mode_char)
            } else {
                format!("-{}", mode_char)
            };

            let mode_msg = Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    "ChanServ".to_string(),
                    "ChanServ".to_string(),
                    "services.".to_string(),
                )),
                command: Command::Raw(
                    "MODE".to_string(),
                    vec![canonical_name.clone(), mode_str.clone(), target_nick.clone()],
                ),
            };

            // Broadcast MODE change to channel members
            matrix.broadcast_to_channel(&channel, mode_msg, None).await;

            info!(channel = %canonical_name, target = %target_nick, mode = %mode_str, "ChanServ mode change");
        }

        ServiceEffect::ForceNick { target_uid, old_nick, new_nick } => {
            // Get user info for NICK message before we modify
            let (username, hostname, channels) = {
                if let Some(user_ref) = matrix.users.get(&target_uid) {
                    let user = user_ref.read().await;
                    (
                        user.user.clone(),
                        user.host.clone(),
                        user.channels.iter().cloned().collect::<Vec<_>>(),
                    )
                } else {
                    return;
                }
            };

            // Update nick mappings
            let old_nick_lower = irc_to_lower(&old_nick);
            let new_nick_lower = irc_to_lower(&new_nick);
            
            matrix.nicks.remove(&old_nick_lower);
            matrix.nicks.insert(new_nick_lower, target_uid.clone());
            
            if let Some(user_ref) = matrix.users.get(&target_uid) {
                let mut user = user_ref.write().await;
                user.nick = new_nick.clone();
            }
            
            // Build NICK message
            let nick_msg = Message {
                tags: None,
                prefix: Some(Prefix::Nickname(old_nick.clone(), username, hostname)),
                command: Command::NICK(new_nick.clone()),
            };

            // Broadcast NICK change to all shared channels
            for channel_name in &channels {
                matrix.broadcast_to_channel(channel_name, nick_msg.clone(), None).await;
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(nick_msg).await;
            }

            info!(uid = %target_uid, old = %old_nick, new = %new_nick, "Forced nick change");
        }
    }
}
