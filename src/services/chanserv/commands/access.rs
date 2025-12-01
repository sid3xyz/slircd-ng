//! Access control ChanServ commands: ACCESS LIST/ADD/DEL.

use super::{format_timestamp, ChanServ, ChanServResult};
use crate::state::Matrix;
use std::sync::Arc;
use tracing::{info, warn};

impl ChanServ {
    /// Handle ACCESS command.
    pub(super) async fn handle_access(
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
    pub(super) async fn handle_access_list(
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
    pub(super) async fn handle_access_add(
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
    pub(super) async fn handle_access_del(
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
}
