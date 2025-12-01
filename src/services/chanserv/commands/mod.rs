//! ChanServ command handlers.
//!
//! This module contains all command handler implementations for ChanServ,
//! organized into submodules by functionality.

mod access;
mod akick;
mod modes;
mod moderation;
mod register;

use crate::db::{ChannelRepository, Database};
use crate::services::ServiceEffect;
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;

/// Result of a ChanServ command - a list of effects to apply.
pub type ChanServResult = Vec<ServiceEffect>;

/// ChanServ service.
pub struct ChanServ {
    pub(crate) db: Database,
}

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
    pub(crate) fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
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
    pub(crate) fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> ChanServResult {
        texts
            .into_iter()
            .map(|t| self.reply_effect(target_uid, t))
            .collect()
    }

    /// Create an error reply.
    pub(crate) fn error_reply(&self, uid: &str, text: &str) -> ChanServResult {
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

    /// Get user's account ID if identified.
    pub(crate) async fn get_user_account_id(&self, matrix: &Arc<Matrix>, uid: &str) -> Option<i64> {
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
    pub(crate) async fn check_founder_access(
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
    pub(crate) fn validate_flags(&self, flags: &str) -> bool {
        // Must start with + and contain only valid flag chars
        if !flags.starts_with('+') {
            return false;
        }

        let flag_chars = &flags[1..];
        flag_chars.chars().all(|c| matches!(c, 'F' | 'o' | 'v'))
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

/// Format a Unix timestamp for display.
pub(crate) fn format_timestamp(ts: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}
