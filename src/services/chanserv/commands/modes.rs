//! Mode change ChanServ commands: OP/DEOP/VOICE/DEVOICE.

use super::{ChanServ, ChanServResult};
use crate::db::ChannelRepository;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tracing::info;

impl ChanServ {
    /// Handle OP/DEOP/VOICE/DEVOICE commands.
    pub(super) async fn handle_mode_change(
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
                tracing::warn!(channel = %channel_name, error = ?e, "Database error");
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
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = channel_ref.send(crate::state::actor::ChannelEvent::GetMemberModes { uid: target_uid.clone(), reply_tx: tx }).await;
            if let Ok(Some(_)) = rx.await {
                true
            } else {
                false
            }
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
}
