//! Registration-related ChanServ commands: REGISTER, DROP, INFO, SET.

use super::{ChanServ, ChanServResult, format_timestamp};
use crate::state::Matrix;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tracing::{info, warn};

impl ChanServ {
    /// Handle REGISTER command.
    pub(super) async fn handle_register(
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

    /// Handle INFO command.
    pub(super) async fn handle_info(&self, uid: &str, args: &[&str]) -> ChanServResult {
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
    pub(super) async fn handle_set(
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
    pub(super) async fn handle_drop(
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
}
