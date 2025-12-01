//! AKICK ChanServ commands: AKICK ADD/DEL/LIST.

use super::{format_timestamp, ChanServ, ChanServResult};
use crate::db::ChannelRepository;
use crate::state::Matrix;
use std::sync::Arc;
use tracing::{info, warn};

impl ChanServ {
    /// Handle AKICK command.
    pub(super) async fn handle_akick(
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
    pub(super) async fn handle_akick_add(
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
    pub(super) async fn handle_akick_del(
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
    pub(super) async fn handle_akick_list(
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
}
