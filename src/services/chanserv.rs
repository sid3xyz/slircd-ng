//! ChanServ - Channel registration and access control service.
//!
//! Handles:
//! - REGISTER #channel [description] - Register a channel
//! - ACCESS #channel LIST - List access entries
//! - ACCESS #channel ADD <account> <flags> - Add access entry
//! - ACCESS #channel DEL <account> - Remove access entry
//! - INFO #channel - Show channel information
//! - SET #channel <option> <value> - Configure channel settings
//! - DROP #channel - Unregister a channel

use crate::db::{ChannelRepository, Database};
use crate::state::Matrix;
use slirc_proto::{irc_to_lower, Command, Message, Prefix};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// ChanServ service.
pub struct ChanServ {
    db: Database,
}

/// Result of a ChanServ command.
pub struct ChanServResult {
    /// Messages to send back to the user.
    pub replies: Vec<Message>,
}

impl ChanServ {
    /// Create a new ChanServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Handle a PRIVMSG to ChanServ.
    pub async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> ChanServResult {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return self.help_reply();
        }

        let command = parts[0].to_uppercase();
        let args = &parts[1..];

        match command.as_str() {
            "REGISTER" => self.handle_register(matrix, uid, nick, args).await,
            "ACCESS" => self.handle_access(matrix, uid, nick, args).await,
            "INFO" => self.handle_info(args).await,
            "SET" => self.handle_set(matrix, uid, nick, args).await,
            "DROP" => self.handle_drop(matrix, uid, nick, args).await,
            "HELP" => self.help_reply(),
            _ => self.unknown_command(&command),
        }
    }

    /// Handle REGISTER command.
    async fn handle_register(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: REGISTER #channel [description]");
        }

        let channel_name = args[0];
        let description = if args.len() > 1 {
            Some(args[1..].join(" "))
        } else {
            None
        };

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply("Channel name must start with #");
        }

        // Check if user is identified
        let account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply("You must be identified to an account to register a channel."),
        };

        // Check if user is op in the channel
        let channel_lower = irc_to_lower(channel_name);
        let is_op = if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
            let channel = channel_ref.read().await;
            channel.is_op(uid)
        } else {
            return self.error_reply(&format!("Channel \x02{}\x02 does not exist.", channel_name));
        };

        if !is_op {
            return self.error_reply(&format!(
                "You must be a channel operator in \x02{}\x02 to register it.",
                channel_name
            ));
        }

        // Register the channel
        match self.db.channels().register(channel_name, account_id, description.as_deref()).await {
            Ok(record) => {
                info!(channel = %channel_name, founder = %nick, "Channel registered");
                ChanServResult {
                    replies: vec![
                        self.notice_msg(&format!(
                            "Channel \x02{}\x02 has been registered under your account.",
                            record.name
                        )),
                    ],
                }
            }
            Err(crate::db::DbError::ChannelExists(name)) => {
                self.error_reply(&format!("Channel \x02{}\x02 is already registered.", name))
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Channel registration failed");
                self.error_reply("Registration failed. Please try again later.")
            }
        }
    }

    /// Handle ACCESS command.
    async fn handle_access(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.len() < 2 {
            return self.error_reply("Syntax: ACCESS #channel <LIST|ADD|DEL> [account] [flags]");
        }

        let channel_name = args[0];
        let subcommand = args[1].to_uppercase();

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply("Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Channel \x02{}\x02 is not registered.",
                    channel_name
                ));
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        match subcommand.as_str() {
            "LIST" => self.handle_access_list(&channel_record).await,
            "ADD" => {
                self.handle_access_add(matrix, uid, nick, &channel_record, &args[2..]).await
            }
            "DEL" => {
                self.handle_access_del(matrix, uid, nick, &channel_record, &args[2..]).await
            }
            _ => self.error_reply("Syntax: ACCESS #channel <LIST|ADD|DEL> [account] [flags]"),
        }
    }

    /// Handle ACCESS LIST subcommand.
    async fn handle_access_list(
        &self,
        channel_record: &crate::db::ChannelRecord,
    ) -> ChanServResult {
        let access_list = match self.db.channels().list_access(channel_record.id).await {
            Ok(list) => list,
            Err(e) => {
                warn!(channel = %channel_record.name, error = ?e, "Failed to list access");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        if access_list.is_empty() {
            return ChanServResult {
                replies: vec![self.notice_msg(&format!(
                    "Access list for \x02{}\x02 is empty.",
                    channel_record.name
                ))],
            };
        }

        let mut replies = vec![self.notice_msg(&format!(
            "Access list for \x02{}\x02:",
            channel_record.name
        ))];

        for (i, entry) in access_list.iter().enumerate() {
            // Look up account name
            let account_name = if let Ok(Some(account)) =
                self.db.accounts().find_by_id(entry.account_id).await
            {
                account.name
            } else {
                format!("(ID:{})", entry.account_id)
            };

            replies.push(self.notice_msg(&format!(
                "  {:>3}. {} ({}) - added by {} on {}",
                i + 1,
                account_name,
                entry.flags,
                entry.added_by,
                format_timestamp(entry.added_at)
            )));
        }

        replies.push(self.notice_msg(&format!(
            "End of access list for \x02{}\x02.",
            channel_record.name
        )));

        ChanServResult { replies }
    }

    /// Handle ACCESS ADD subcommand.
    async fn handle_access_add(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        channel_record: &crate::db::ChannelRecord,
        args: &[&str],
    ) -> ChanServResult {
        if args.len() < 2 {
            return self.error_reply("Syntax: ACCESS #channel ADD <account> <flags>");
        }

        let target_account_name = args[0];
        let flags = args[1];

        // Check if user has founder access
        if !self.check_founder_access(matrix, uid, channel_record).await {
            return self.error_reply("You must be the channel founder to modify access.");
        }

        // Find target account
        let target_account = match self.db.accounts().find_by_name(target_account_name).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Account \x02{}\x02 does not exist.",
                    target_account_name
                ));
            }
            Err(e) => {
                warn!(account = %target_account_name, error = ?e, "Failed to find account");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        // Validate flags
        if !self.validate_flags(flags) {
            return self.error_reply("Invalid flags. Valid flags: +F (founder), +o (op), +v (voice)");
        }

        // Add access
        if let Err(e) = self
            .db
            .channels()
            .set_access(channel_record.id, target_account.id, flags, nick)
            .await
        {
            warn!(channel = %channel_record.name, account = %target_account_name, error = ?e, "Failed to set access");
            return self.error_reply("Failed to add access. Please try again later.");
        }

        info!(
            channel = %channel_record.name,
            account = %target_account_name,
            flags = %flags,
            by = %nick,
            "Access added"
        );

        ChanServResult {
            replies: vec![self.notice_msg(&format!(
                "Access for \x02{}\x02 on \x02{}\x02 set to \x02{}\x02.",
                target_account_name, channel_record.name, flags
            ))],
        }
    }

    /// Handle ACCESS DEL subcommand.
    async fn handle_access_del(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        channel_record: &crate::db::ChannelRecord,
        args: &[&str],
    ) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: ACCESS #channel DEL <account>");
        }

        let target_account_name = args[0];

        // Check if user has founder access
        if !self.check_founder_access(matrix, uid, channel_record).await {
            return self.error_reply("You must be the channel founder to modify access.");
        }

        // Find target account
        let target_account = match self.db.accounts().find_by_name(target_account_name).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Account \x02{}\x02 does not exist.",
                    target_account_name
                ));
            }
            Err(e) => {
                warn!(account = %target_account_name, error = ?e, "Failed to find account");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        // Cannot remove founder
        if target_account.id == channel_record.founder_account_id {
            return self.error_reply("Cannot remove founder access from the channel owner.");
        }

        // Remove access
        match self
            .db
            .channels()
            .remove_access(channel_record.id, target_account.id)
            .await
        {
            Ok(true) => {
                info!(
                    channel = %channel_record.name,
                    account = %target_account_name,
                    by = %nick,
                    "Access removed"
                );
                ChanServResult {
                    replies: vec![self.notice_msg(&format!(
                        "Access for \x02{}\x02 on \x02{}\x02 has been removed.",
                        target_account_name, channel_record.name
                    ))],
                }
            }
            Ok(false) => self.error_reply(&format!(
                "\x02{}\x02 does not have access on \x02{}\x02.",
                target_account_name, channel_record.name
            )),
            Err(e) => {
                warn!(channel = %channel_record.name, account = %target_account_name, error = ?e, "Failed to remove access");
                self.error_reply("Failed to remove access. Please try again later.")
            }
        }
    }

    /// Handle INFO command.
    async fn handle_info(&self, args: &[&str]) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: INFO #channel");
        }

        let channel_name = args[0];

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply("Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Channel \x02{}\x02 is not registered.",
                    channel_name
                ));
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        // Get founder account name
        let founder_name = if let Ok(Some(account)) = self
            .db
            .accounts()
            .find_by_id(channel_record.founder_account_id)
            .await
        {
            account.name
        } else {
            "(unknown)".to_string()
        };

        let mut replies = vec![
            self.notice_msg(&format!("Information for \x02{}\x02:", channel_record.name)),
            self.notice_msg(&format!("  Founder    : {}", founder_name)),
            self.notice_msg(&format!(
                "  Registered : {}",
                format_timestamp(channel_record.registered_at)
            )),
            self.notice_msg(&format!(
                "  Last used  : {}",
                format_timestamp(channel_record.last_used_at)
            )),
        ];

        if let Some(ref desc) = channel_record.description {
            replies.push(self.notice_msg(&format!("  Description: {}", desc)));
        }

        if let Some(ref mlock) = channel_record.mlock {
            replies.push(self.notice_msg(&format!("  Mode lock  : {}", mlock)));
        }

        replies.push(self.notice_msg(&format!(
            "  Keep topic : {}",
            if channel_record.keeptopic { "ON" } else { "OFF" }
        )));

        replies.push(self.notice_msg(&format!(
            "End of info for \x02{}\x02.",
            channel_record.name
        )));

        ChanServResult { replies }
    }

    /// Handle SET command.
    async fn handle_set(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.len() < 3 {
            return self.error_reply("Syntax: SET #channel <option> <value>");
        }

        let channel_name = args[0];
        let option = args[1];
        let value = args[2..].join(" ");

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply("Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Channel \x02{}\x02 is not registered.",
                    channel_name
                ));
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        // Check if user has founder access
        if !self.check_founder_access(matrix, uid, &channel_record).await {
            return self.error_reply("You must be the channel founder to change settings.");
        }

        // Update setting
        match self
            .db
            .channels()
            .set_option(channel_record.id, option, &value)
            .await
        {
            Ok(()) => {
                info!(
                    channel = %channel_name,
                    option = %option,
                    value = %value,
                    by = %nick,
                    "Channel setting updated"
                );
                ChanServResult {
                    replies: vec![self.notice_msg(&format!(
                        "Setting \x02{}\x02 for \x02{}\x02 has been set to \x02{}\x02.",
                        option, channel_name, value
                    ))],
                }
            }
            Err(crate::db::DbError::UnknownOption(opt)) => {
                self.error_reply(&format!("Unknown option: \x02{}\x02. Valid options: description, mlock, keeptopic", opt))
            }
            Err(e) => {
                warn!(channel = %channel_name, option = %option, error = ?e, "Failed to set option");
                self.error_reply("Failed to update setting. Please try again later.")
            }
        }
    }

    /// Handle DROP command.
    async fn handle_drop(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply("Syntax: DROP #channel");
        }

        let channel_name = args[0];

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply("Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(&format!(
                    "Channel \x02{}\x02 is not registered.",
                    channel_name
                ));
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply("Database error. Please try again later.");
            }
        };

        // Check if user is founder (strict - only founder can drop)
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply("You must be identified to your account."),
        };

        if user_account_id != channel_record.founder_account_id {
            return self.error_reply("Only the channel founder can drop this channel.");
        }

        // Drop the channel
        match self.db.channels().drop_channel(channel_record.id).await {
            Ok(true) => {
                info!(channel = %channel_name, by = %nick, "Channel dropped");
                ChanServResult {
                    replies: vec![self.notice_msg(&format!(
                        "Channel \x02{}\x02 has been dropped.",
                        channel_name
                    ))],
                }
            }
            Ok(false) => self.error_reply("Failed to drop channel."),
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to drop channel");
                self.error_reply("Failed to drop channel. Please try again later.")
            }
        }
    }

    /// Get user's account ID if identified.
    async fn get_user_account_id(&self, matrix: &Arc<Matrix>, uid: &str) -> Option<i64> {
        let user = matrix.users.get(uid)?;
        let user = user.read().await;

        if !user.modes.registered {
            return None;
        }

        let account_name = user.account.as_ref()?;

        // Look up account ID
        match self.db.accounts().find_by_name(account_name).await {
            Ok(Some(account)) => Some(account.id),
            _ => None,
        }
    }

    /// Check if user has founder access on a channel.
    async fn check_founder_access(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        channel_record: &crate::db::ChannelRecord,
    ) -> bool {
        let account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return false,
        };

        // Check if user is founder
        if account_id == channel_record.founder_account_id {
            return true;
        }

        // Check access list for +F flag
        if let Ok(Some(access)) = self
            .db
            .channels()
            .get_access(channel_record.id, account_id)
            .await
        {
            return ChannelRepository::is_founder(&access.flags);
        }

        false
    }

    /// Validate access flags.
    fn validate_flags(&self, flags: &str) -> bool {
        // Must start with + and contain only valid flag chars
        if !flags.starts_with('+') {
            return false;
        }

        let flag_chars = &flags[1..];
        flag_chars.chars().all(|c| matches!(c, 'F' | 'o' | 'v'))
    }

    /// Create a NOTICE message from ChanServ.
    fn notice_msg(&self, text: &str) -> Message {
        Message {
            tags: None,
            prefix: Some(Prefix::ServerName("ChanServ".to_string())),
            command: Command::NOTICE("*".to_string(), text.to_string()),
        }
    }

    /// Create an error reply.
    fn error_reply(&self, text: &str) -> ChanServResult {
        ChanServResult {
            replies: vec![self.notice_msg(text)],
        }
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, cmd: &str) -> ChanServResult {
        ChanServResult {
            replies: vec![self.notice_msg(&format!(
                "Unknown command: \x02{}\x02. Use \x02HELP\x02 for a list of commands.",
                cmd
            ))],
        }
    }

    /// Create help reply.
    fn help_reply(&self) -> ChanServResult {
        ChanServResult {
            replies: vec![
                self.notice_msg("***** ChanServ Help *****"),
                self.notice_msg("ChanServ allows you to register and manage channels."),
                self.notice_msg(" "),
                self.notice_msg("Available commands:"),
                self.notice_msg("  REGISTER #channel [description] - Register a channel"),
                self.notice_msg("  ACCESS #channel LIST            - List access entries"),
                self.notice_msg("  ACCESS #channel ADD <acct> <flags> - Add access"),
                self.notice_msg("  ACCESS #channel DEL <account>   - Remove access"),
                self.notice_msg("  INFO #channel                   - Show channel info"),
                self.notice_msg("  SET #channel <opt> <value>      - Change settings"),
                self.notice_msg("  DROP #channel                   - Unregister channel"),
                self.notice_msg(" "),
                self.notice_msg("Access flags: +F (founder), +o (auto-op), +v (auto-voice)"),
                self.notice_msg("***** End of Help *****"),
            ],
        }
    }
}

/// Route a service message to ChanServ.
pub async fn route_chanserv_message(
    matrix: &Arc<Matrix>,
    db: &Database,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &mpsc::Sender<Message>,
) -> bool {
    let target_lower = irc_to_lower(target);

    if target_lower == "chanserv" || target_lower == "cs" {
        let chanserv = ChanServ::new(db.clone());
        let result = chanserv.handle(matrix, uid, nick, text).await;

        // Send replies
        for mut reply in result.replies {
            // Set the target for the NOTICE
            if let Command::NOTICE(_, text) = &reply.command {
                reply.command = Command::NOTICE(nick.to_string(), text.clone());
            }
            let _ = sender.send(reply).await;
        }

        true
    } else {
        false
    }
}

/// Format a Unix timestamp for display.
fn format_timestamp(ts: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "(unknown)".to_string())
}
