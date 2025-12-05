//! Moderation ChanServ commands: CLEAR USERS.

use super::{ChanServ, ChanServResult};
use crate::services::ServiceEffect;
use crate::state::Matrix;
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use tracing::{info, warn};

impl ChanServ {
    /// Handle CLEAR command - mass-kick users from a channel.
    ///
    /// `CLEAR #channel USERS [reason]`
    ///
    /// Kicks all users without +o (operator) status from the channel.
    /// Requires +F (founder) access on the channel.
    pub(super) async fn handle_clear(
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
                &format!(
                    "Unknown CLEAR subcommand: \x02{}\x02. Use: CLEAR #channel USERS [reason]",
                    subcommand
                ),
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
            return self.error_reply(uid, "You need +F (founder) access to use CLEAR.");
        }

        // Get channel state and collect UIDs to kick
        let channel_lower = irc_to_lower(channel_name);
        let users_to_kick: Vec<String> = {
            if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = channel_ref.send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx }).await;
                if let Ok(members) = rx.await {
                    members
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
}
