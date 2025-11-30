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
use crate::services::ServiceEffect;
use crate::services::nickserv::apply_effect;
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix, irc_to_lower};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// ChanServ service.
pub struct ChanServ {
    db: Database,
}

/// Result of a ChanServ command - a list of effects to apply.
pub type ChanServResult = Vec<ServiceEffect>;

impl ChanServ {
    /// Create a new ChanServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Handle a PRIVMSG to ChanServ.
    /// Returns a list of effects that the caller should apply.
    pub async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> ChanServResult {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return self.help_reply(uid);
        }

        let command = parts[0].to_uppercase();
        let args = &parts[1..];

        match command.as_str() {
            "REGISTER" => self.handle_register(matrix, uid, nick, args).await,
            "ACCESS" => self.handle_access(matrix, uid, nick, args).await,
            "INFO" => self.handle_info(uid, args).await,
            "SET" => self.handle_set(matrix, uid, nick, args).await,
            "DROP" => self.handle_drop(matrix, uid, nick, args).await,
            "OP" => self.handle_mode_change(matrix, uid, nick, args, "+o").await,
            "DEOP" => self.handle_mode_change(matrix, uid, nick, args, "-o").await,
            "VOICE" => self.handle_mode_change(matrix, uid, nick, args, "+v").await,
            "DEVOICE" => self.handle_mode_change(matrix, uid, nick, args, "-v").await,
            "AKICK" => self.handle_akick(matrix, uid, nick, args).await,
            "CLEAR" => self.handle_clear(matrix, uid, nick, args).await,
            "HELP" => self.help_reply(uid),
            _ => self.unknown_command(uid, &command),
        }
    }

    // ========== Helper methods for creating effects ==========

    /// Create a single reply effect.
    fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: target_uid.to_string(),
            msg: Message {
                tags: None,
                prefix: Some(Prefix::ServerName("ChanServ".to_string())),
                command: Command::NOTICE("*".to_string(), text.to_string()),
            },
        }
    }

    /// Create multiple reply effects.
    fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> ChanServResult {
        texts
            .into_iter()
            .map(|t| self.reply_effect(target_uid, t))
            .collect()
    }

    /// Create an error reply.
    fn error_reply(&self, uid: &str, text: &str) -> ChanServResult {
        vec![self.reply_effect(uid, text)]
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, uid: &str, cmd: &str) -> ChanServResult {
        self.error_reply(
            uid,
            &format!(
                "Unknown command: \x02{}\x02. Use \x02HELP\x02 for a list of commands.",
                cmd
            ),
        )
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
            return self.error_reply(uid, "Syntax: REGISTER #channel [description]");
        }

        let channel_name = args[0];
        let description = if args.len() > 1 {
            Some(args[1..].join(" "))
        } else {
            None
        };

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Check if user is identified
        let account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => {
                return self.error_reply(
                    uid,
                    "You must be identified to an account to register a channel.",
                );
            }
        };

        // Check if user is op in the channel
        let channel_lower = irc_to_lower(channel_name);
        let is_op = if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
            let channel = channel_ref.read().await;
            channel.is_op(uid)
        } else {
            return self.error_reply(
                uid,
                &format!("Channel \x02{}\x02 does not exist.", channel_name),
            );
        };

        if !is_op {
            return self.error_reply(
                uid,
                &format!(
                    "You must be a channel operator in \x02{}\x02 to register it.",
                    channel_name
                ),
            );
        }

        // Register the channel
        match self
            .db
            .channels()
            .register(channel_name, account_id, description.as_deref())
            .await
        {
            Ok(record) => {
                info!(channel = %channel_name, founder = %nick, "Channel registered");
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "Channel \x02{}\x02 has been registered under your account.",
                        record.name
                    )],
                )
            }
            Err(crate::db::DbError::ChannelExists(name)) => self.error_reply(
                uid,
                &format!("Channel \x02{}\x02 is already registered.", name),
            ),
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Channel registration failed");
                self.error_reply(uid, "Registration failed. Please try again later.")
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
            return self.error_reply(
                uid,
                "Syntax: ACCESS #channel <LIST|ADD|DEL> [account] [flags]",
            );
        }

        let channel_name = args[0];
        let subcommand = args[1].to_uppercase();

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        match subcommand.as_str() {
            "LIST" => self.handle_access_list(uid, &channel_record).await,
            "ADD" => {
                self.handle_access_add(matrix, uid, nick, &channel_record, &args[2..])
                    .await
            }
            "DEL" => {
                self.handle_access_del(matrix, uid, nick, &channel_record, &args[2..])
                    .await
            }
            _ => self.error_reply(
                uid,
                "Syntax: ACCESS #channel <LIST|ADD|DEL> [account] [flags]",
            ),
        }
    }

    /// Handle ACCESS LIST subcommand.
    async fn handle_access_list(
        &self,
        uid: &str,
        channel_record: &crate::db::ChannelRecord,
    ) -> ChanServResult {
        let access_list = match self.db.channels().list_access(channel_record.id).await {
            Ok(list) => list,
            Err(e) => {
                warn!(channel = %channel_record.name, error = ?e, "Failed to list access");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        if access_list.is_empty() {
            return self.reply_effects(
                uid,
                vec![&format!(
                    "Access list for \x02{}\x02 is empty.",
                    channel_record.name
                )],
            );
        }

        let mut texts = vec![format!("Access list for \x02{}\x02:", channel_record.name)];

        for (i, entry) in access_list.iter().enumerate() {
            // Look up account name
            let account_name =
                if let Ok(Some(account)) = self.db.accounts().find_by_id(entry.account_id).await {
                    account.name
                } else {
                    format!("(ID:{})", entry.account_id)
                };

            texts.push(format!(
                "  {:>3}. {} ({}) - added by {} on {}",
                i + 1,
                account_name,
                entry.flags,
                entry.added_by,
                format_timestamp(entry.added_at)
            ));
        }

        texts.push(format!(
            "End of access list for \x02{}\x02.",
            channel_record.name
        ));

        texts.iter().map(|t| self.reply_effect(uid, t)).collect()
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
            return self.error_reply(uid, "Syntax: ACCESS #channel ADD <account> <flags>");
        }

        let target_account_name = args[0];
        let flags = args[1];

        // Check if user has founder access
        if !self.check_founder_access(matrix, uid, channel_record).await {
            return self.error_reply(uid, "You must be the channel founder to modify access.");
        }

        // Find target account
        let target_account = match self.db.accounts().find_by_name(target_account_name).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Account \x02{}\x02 does not exist.", target_account_name),
                );
            }
            Err(e) => {
                warn!(account = %target_account_name, error = ?e, "Failed to find account");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Validate flags
        if !self.validate_flags(flags) {
            return self.error_reply(
                uid,
                "Invalid flags. Valid flags: +F (founder), +o (op), +v (voice)",
            );
        }

        // Add access
        if let Err(e) = self
            .db
            .channels()
            .set_access(channel_record.id, target_account.id, flags, nick)
            .await
        {
            warn!(channel = %channel_record.name, account = %target_account_name, error = ?e, "Failed to set access");
            return self.error_reply(uid, "Failed to add access. Please try again later.");
        }

        info!(
            channel = %channel_record.name,
            account = %target_account_name,
            flags = %flags,
            by = %nick,
            "Access added"
        );

        self.reply_effects(
            uid,
            vec![&format!(
                "Access for \x02{}\x02 on \x02{}\x02 set to \x02{}\x02.",
                target_account_name, channel_record.name, flags
            )],
        )
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
            return self.error_reply(uid, "Syntax: ACCESS #channel DEL <account>");
        }

        let target_account_name = args[0];

        // Check if user has founder access
        if !self.check_founder_access(matrix, uid, channel_record).await {
            return self.error_reply(uid, "You must be the channel founder to modify access.");
        }

        // Find target account
        let target_account = match self.db.accounts().find_by_name(target_account_name).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Account \x02{}\x02 does not exist.", target_account_name),
                );
            }
            Err(e) => {
                warn!(account = %target_account_name, error = ?e, "Failed to find account");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Cannot remove founder
        if target_account.id == channel_record.founder_account_id {
            return self.error_reply(uid, "Cannot remove founder access from the channel owner.");
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
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "Access for \x02{}\x02 on \x02{}\x02 has been removed.",
                        target_account_name, channel_record.name
                    )],
                )
            }
            Ok(false) => self.error_reply(
                uid,
                &format!(
                    "\x02{}\x02 does not have access on \x02{}\x02.",
                    target_account_name, channel_record.name
                ),
            ),
            Err(e) => {
                warn!(channel = %channel_record.name, account = %target_account_name, error = ?e, "Failed to remove access");
                self.error_reply(uid, "Failed to remove access. Please try again later.")
            }
        }
    }

    /// Handle INFO command.
    async fn handle_info(&self, uid: &str, args: &[&str]) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply(uid, "Syntax: INFO #channel");
        }

        let channel_name = args[0];

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply(uid, "Database error. Please try again later.");
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

        let mut texts = vec![
            format!("Information for \x02{}\x02:", channel_record.name),
            format!("  Founder    : {}", founder_name),
            format!(
                "  Registered : {}",
                format_timestamp(channel_record.registered_at)
            ),
            format!(
                "  Last used  : {}",
                format_timestamp(channel_record.last_used_at)
            ),
        ];

        if let Some(ref desc) = channel_record.description {
            texts.push(format!("  Description: {}", desc));
        }

        if let Some(ref mlock) = channel_record.mlock {
            texts.push(format!("  Mode lock  : {}", mlock));
        }

        texts.push(format!(
            "  Keep topic : {}",
            if channel_record.keeptopic {
                "ON"
            } else {
                "OFF"
            }
        ));

        texts.push(format!("End of info for \x02{}\x02.", channel_record.name));

        texts.iter().map(|t| self.reply_effect(uid, t)).collect()
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
            return self.error_reply(uid, "Syntax: SET #channel <option> <value>");
        }

        let channel_name = args[0];
        let option = args[1];
        let value = args[2..].join(" ");

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Check if user has founder access
        if !self
            .check_founder_access(matrix, uid, &channel_record)
            .await
        {
            return self.error_reply(uid, "You must be the channel founder to change settings.");
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
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "Setting \x02{}\x02 for \x02{}\x02 has been set to \x02{}\x02.",
                        option, channel_name, value
                    )],
                )
            }
            Err(crate::db::DbError::UnknownOption(opt)) => self.error_reply(
                uid,
                &format!(
                    "Unknown option: \x02{}\x02. Valid options: description, mlock, keeptopic",
                    opt
                ),
            ),
            Err(e) => {
                warn!(channel = %channel_name, option = %option, error = ?e, "Failed to set option");
                self.error_reply(uid, "Failed to update setting. Please try again later.")
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
            return self.error_reply(uid, "Syntax: DROP #channel");
        }

        let channel_name = args[0];

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Find registered channel
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to lookup channel");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Check if user is founder (strict - only founder can drop)
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply(uid, "You must be identified to your account."),
        };

        if user_account_id != channel_record.founder_account_id {
            return self.error_reply(uid, "Only the channel founder can drop this channel.");
        }

        // Drop the channel
        match self.db.channels().drop_channel(channel_record.id).await {
            Ok(true) => {
                info!(channel = %channel_name, by = %nick, "Channel dropped");
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "Channel \x02{}\x02 has been dropped.",
                        channel_name
                    )],
                )
            }
            Ok(false) => self.error_reply(uid, "Failed to drop channel."),
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Failed to drop channel");
                self.error_reply(uid, "Failed to drop channel. Please try again later.")
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

    /// Handle AKICK command.
    async fn handle_akick(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.len() < 2 {
            return self.error_reply(uid, "Syntax: AKICK #channel <ADD|DEL|LIST> [mask] [reason]");
        }

        let channel_name = args[0];
        let subcommand = args[1].to_uppercase();

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Get the channel record
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Database error");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        match subcommand.as_str() {
            "ADD" => {
                self.handle_akick_add(matrix, uid, nick, &channel_record, &args[2..])
                    .await
            }
            "DEL" => {
                self.handle_akick_del(matrix, uid, nick, &channel_record, &args[2..])
                    .await
            }
            "LIST" => self.handle_akick_list(uid, &channel_record).await,
            _ => self.error_reply(uid, "Unknown subcommand. Valid: ADD, DEL, LIST"),
        }
    }

    /// Handle AKICK ADD subcommand.
    async fn handle_akick_add(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        channel_record: &crate::db::ChannelRecord,
        args: &[&str],
    ) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply(uid, "Syntax: AKICK #channel ADD <mask> [reason]");
        }

        let mask = args[0];
        let reason = if args.len() > 1 {
            Some(args[1..].join(" "))
        } else {
            None
        };

        // Check if user has op access
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply(uid, "You must be identified to your account."),
        };

        let has_access = if user_account_id == channel_record.founder_account_id {
            true
        } else if let Ok(Some(access)) = self
            .db
            .channels()
            .get_access(channel_record.id, user_account_id)
            .await
        {
            ChannelRepository::has_op_access(&access.flags)
        } else {
            false
        };

        if !has_access {
            return self.error_reply(uid, "You do not have access to modify the AKICK list.");
        }

        // Add the AKICK
        match self
            .db
            .channels()
            .add_akick(channel_record.id, mask, reason.as_deref(), nick)
            .await
        {
            Ok(()) => {
                info!(
                    channel = %channel_record.name,
                    mask = %mask,
                    by = %nick,
                    "AKICK added"
                );
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "AKICK for \x02{}\x02 added to \x02{}\x02.",
                        mask, channel_record.name
                    )],
                )
            }
            Err(e) => {
                warn!(channel = %channel_record.name, mask = %mask, error = ?e, "Failed to add AKICK");
                self.error_reply(uid, "Failed to add AKICK. Please try again later.")
            }
        }
    }

    /// Handle AKICK DEL subcommand.
    async fn handle_akick_del(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        channel_record: &crate::db::ChannelRecord,
        args: &[&str],
    ) -> ChanServResult {
        if args.is_empty() {
            return self.error_reply(uid, "Syntax: AKICK #channel DEL <mask>");
        }

        let mask = args[0];

        // Check if user has op access
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply(uid, "You must be identified to your account."),
        };

        let has_access = if user_account_id == channel_record.founder_account_id {
            true
        } else if let Ok(Some(access)) = self
            .db
            .channels()
            .get_access(channel_record.id, user_account_id)
            .await
        {
            ChannelRepository::has_op_access(&access.flags)
        } else {
            false
        };

        if !has_access {
            return self.error_reply(uid, "You do not have access to modify the AKICK list.");
        }

        // Remove the AKICK
        match self
            .db
            .channels()
            .remove_akick(channel_record.id, mask)
            .await
        {
            Ok(true) => {
                info!(
                    channel = %channel_record.name,
                    mask = %mask,
                    by = %nick,
                    "AKICK removed"
                );
                self.reply_effects(
                    uid,
                    vec![&format!(
                        "AKICK for \x02{}\x02 removed from \x02{}\x02.",
                        mask, channel_record.name
                    )],
                )
            }
            Ok(false) => self.error_reply(
                uid,
                &format!("No AKICK entry matching \x02{}\x02 found.", mask),
            ),
            Err(e) => {
                warn!(channel = %channel_record.name, mask = %mask, error = ?e, "Failed to remove AKICK");
                self.error_reply(uid, "Failed to remove AKICK. Please try again later.")
            }
        }
    }

    /// Handle AKICK LIST subcommand.
    async fn handle_akick_list(
        &self,
        uid: &str,
        channel_record: &crate::db::ChannelRecord,
    ) -> ChanServResult {
        let akicks = match self.db.channels().list_akicks(channel_record.id).await {
            Ok(list) => list,
            Err(e) => {
                warn!(channel = %channel_record.name, error = ?e, "Failed to list AKICKs");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        if akicks.is_empty() {
            return self.reply_effects(
                uid,
                vec![&format!(
                    "AKICK list for \x02{}\x02 is empty.",
                    channel_record.name
                )],
            );
        }

        let mut texts = vec![format!("AKICK list for \x02{}\x02:", channel_record.name)];

        for (i, akick) in akicks.iter().enumerate() {
            let reason = akick.reason.as_deref().unwrap_or("(no reason)");
            texts.push(format!(
                "  {:>3}. {} - {} (by {} on {})",
                i + 1,
                akick.mask,
                reason,
                akick.set_by,
                format_timestamp(akick.set_at)
            ));
        }

        texts.push(format!(
            "End of AKICK list for \x02{}\x02.",
            channel_record.name
        ));

        texts.iter().map(|t| self.reply_effect(uid, t)).collect()
    }

    /// Handle OP/DEOP/VOICE/DEVOICE commands.
    async fn handle_mode_change(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
        mode: &str,
    ) -> ChanServResult {
        let cmd_name = match mode {
            "+o" => "OP",
            "-o" => "DEOP",
            "+v" => "VOICE",
            "-v" => "DEVOICE",
            _ => "MODE",
        };

        if args.is_empty() {
            return self.error_reply(uid, &format!("Syntax: {} #channel [nick]", cmd_name));
        }

        let channel_name = args[0];
        // Default to self if no target specified
        let target_nick = if args.len() > 1 { args[1] } else { nick };

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        let channel_lower = irc_to_lower(channel_name);

        // Get the channel record
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Database error");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Check if user has op access (+o or +F)
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply(uid, "You must be identified to your account."),
        };

        // Check if founder or has +o flag
        let has_access = if user_account_id == channel_record.founder_account_id {
            true
        } else if let Ok(Some(access)) = self
            .db
            .channels()
            .get_access(channel_record.id, user_account_id)
            .await
        {
            ChannelRepository::has_op_access(&access.flags)
        } else {
            false
        };

        if !has_access {
            return self.error_reply(
                uid,
                &format!(
                    "You do not have access to use {} on \x02{}\x02.",
                    cmd_name, channel_name
                ),
            );
        }

        // Verify target is in the channel
        let target_nick_lower = irc_to_lower(target_nick);
        let target_uid = match matrix.nicks.get(&target_nick_lower) {
            Some(uid_ref) => uid_ref.clone(),
            None => {
                return self.error_reply(uid, &format!("\x02{}\x02 is not online.", target_nick));
            }
        };

        // Check if target is in channel
        let in_channel = if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
            let channel = channel_ref.read().await;
            channel.is_member(&target_uid)
        } else {
            return self.error_reply(
                uid,
                &format!("Channel \x02{}\x02 does not exist.", channel_name),
            );
        };

        if !in_channel {
            return self.error_reply(
                uid,
                &format!(
                    "\x02{}\x02 is not in \x02{}\x02.",
                    target_nick, channel_name
                ),
            );
        }

        // Extract mode_char and adding from mode string (e.g., "+o" -> 'o', true)
        let adding = mode.starts_with('+');
        let mode_char = mode.chars().nth(1).unwrap_or('o');

        info!(
            channel = %channel_name,
            target = %target_nick,
            mode = %mode,
            by = %nick,
            "ChanServ mode change"
        );

        vec![
            self.reply_effect(
                uid,
                &format!("Mode {} {} on \x02{}\x02.", mode, target_nick, channel_name),
            ),
            ServiceEffect::ChannelMode {
                channel: channel_lower,
                target_uid,
                mode_char,
                adding,
            },
        ]
    }

    /// Handle CLEAR command - mass-kick users from a channel.
    ///
    /// CLEAR #channel USERS [reason]
    ///
    /// Kicks all users without +o (operator) status from the channel.
    /// Requires +F (founder) access on the channel.
    async fn handle_clear(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        args: &[&str],
    ) -> ChanServResult {
        if args.len() < 2 {
            return self.error_reply(uid, "Syntax: CLEAR #channel USERS [reason]");
        }

        let channel_name = args[0];
        let subcommand = args[1].to_uppercase();

        // Validate channel name
        if !channel_name.starts_with('#') {
            return self.error_reply(uid, "Channel name must start with #");
        }

        // Only USERS subcommand for now
        if subcommand != "USERS" {
            return self.error_reply(
                uid,
                &format!("Unknown CLEAR subcommand: \x02{}\x02. Use: CLEAR #channel USERS [reason]", subcommand),
            );
        }

        let reason = if args.len() > 2 {
            args[2..].join(" ")
        } else {
            "Channel cleared".to_string()
        };

        // Check if channel is registered
        let channel_record = match self.db.channels().find_by_name(channel_name).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 is not registered.", channel_name),
                );
            }
            Err(e) => {
                warn!(channel = %channel_name, error = ?e, "Database error checking channel");
                return self.error_reply(uid, "Database error. Please try again later.");
            }
        };

        // Check if user has founder access (+F)
        let user_account_id = match self.get_user_account_id(matrix, uid).await {
            Some(id) => id,
            None => return self.error_reply(uid, "You must be identified to your account."),
        };

        let is_founder = user_account_id == channel_record.founder_account_id;
        let has_founder_flag = if !is_founder {
            if let Ok(Some(access)) = self
                .db
                .channels()
                .get_access(channel_record.id, user_account_id)
                .await
            {
                access.flags.contains('F')
            } else {
                false
            }
        } else {
            true
        };

        if !has_founder_flag {
            return self.error_reply(
                uid,
                "You need +F (founder) access to use CLEAR.",
            );
        }

        // Get channel state and collect UIDs to kick
        let channel_lower = irc_to_lower(channel_name);
        let users_to_kick: Vec<String> = {
            if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                let channel = channel_ref.read().await;
                channel
                    .members
                    .iter()
                    .filter(|(member_uid, modes)| {
                        // Don't kick opped users or the person running the command
                        !modes.op && *member_uid != uid
                    })
                    .map(|(member_uid, _)| member_uid.clone())
                    .collect()
            } else {
                return self.error_reply(
                    uid,
                    &format!("Channel \x02{}\x02 does not exist.", channel_name),
                );
            }
        };

        if users_to_kick.is_empty() {
            return self.reply_effects(
                uid,
                vec!["No users to kick (all users have +o or you're alone)."],
            );
        }

        info!(
            channel = %channel_name,
            by = %nick,
            count = users_to_kick.len(),
            "ChanServ CLEAR USERS executed"
        );

        // Build kick effects
        let mut effects: Vec<ServiceEffect> = vec![self.reply_effect(
            uid,
            &format!(
                "Clearing \x02{}\x02 users from \x02{}\x02...",
                users_to_kick.len(),
                channel_name
            ),
        )];

        for target_uid in users_to_kick {
            effects.push(ServiceEffect::Kick {
                channel: channel_name.to_string(),
                target_uid,
                kicker: "ChanServ".to_string(),
                reason: reason.clone(),
            });
        }

        effects.push(self.reply_effect(
            uid,
            &format!("Channel \x02{}\x02 has been cleared.", channel_name),
        ));

        effects
    }

    /// Create help reply.
    fn help_reply(&self, uid: &str) -> ChanServResult {
        vec![
            self.reply_effect(uid, "***** ChanServ Help *****"),
            self.reply_effect(uid, "ChanServ allows you to register and manage channels."),
            self.reply_effect(uid, " "),
            self.reply_effect(uid, "Available commands:"),
            self.reply_effect(
                uid,
                "  REGISTER #channel [description] - Register a channel",
            ),
            self.reply_effect(
                uid,
                "  ACCESS #channel LIST            - List access entries",
            ),
            self.reply_effect(uid, "  ACCESS #channel ADD <acct> <flags> - Add access"),
            self.reply_effect(uid, "  ACCESS #channel DEL <account>   - Remove access"),
            self.reply_effect(uid, "  AKICK #channel ADD <mask> [reason] - Add auto-kick"),
            self.reply_effect(uid, "  AKICK #channel DEL <mask>       - Remove auto-kick"),
            self.reply_effect(uid, "  AKICK #channel LIST             - List auto-kicks"),
            self.reply_effect(uid, "  CLEAR #channel USERS [reason]   - Kick non-opped users"),
            self.reply_effect(uid, "  INFO #channel                   - Show channel info"),
            self.reply_effect(uid, "  SET #channel <opt> <value>      - Change settings"),
            self.reply_effect(
                uid,
                "  DROP #channel                   - Unregister channel",
            ),
            self.reply_effect(uid, "  OP #channel [nick]              - Give channel ops"),
            self.reply_effect(
                uid,
                "  DEOP #channel [nick]            - Remove channel ops",
            ),
            self.reply_effect(uid, "  VOICE #channel [nick]           - Give voice"),
            self.reply_effect(uid, "  DEVOICE #channel [nick]         - Remove voice"),
            self.reply_effect(uid, " "),
            self.reply_effect(
                uid,
                "Access flags: +F (founder), +o (auto-op), +v (auto-voice)",
            ),
            self.reply_effect(uid, "***** End of Help *****"),
        ]
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
        let effects = chanserv.handle(matrix, uid, nick, text).await;

        // Apply each effect
        for effect in effects {
            apply_effect(matrix, nick, sender, effect).await;
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
