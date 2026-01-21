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

        // Common setup for all commands
        let channel_sender = if let Some(c) = matrix.channel_manager.channels.get(&channel_lower) {
            c.value().clone()
        } else {
            return self.error_reply(
                uid,
                &format!("Channel \x02{}\x02 does not exist.", channel_name),
            );
        };

        let sender_prefix = slirc_proto::Prefix::new(
            "ChanServ".to_string(),
            "ChanServ".to_string(),
            "services.".to_string(),
        );

        match subcommand.as_str() {
            "USERS" => {
                let users_to_kick: Vec<String> = {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::GetMembers { reply_tx: tx })
                        .await;
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
            "MODES" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let event = crate::state::actor::ChannelEvent::Clear {
                    sender_uid: "ChanServ".to_string(),
                    sender_prefix,
                    target: crate::state::actor::ClearTarget::Modes,
                    nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                    reply_tx: tx,
                };
                let _ = channel_sender.send(event).await;
                let _ = rx.await;
                vec![self.reply_effect(uid, &format!("Cleared modes on \x02{}\x02.", channel_name))]
            }
            "BANS" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let event = crate::state::actor::ChannelEvent::Clear {
                    sender_uid: "ChanServ".to_string(),
                    sender_prefix,
                    target: crate::state::actor::ClearTarget::Bans,
                    nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                    reply_tx: tx,
                };
                let _ = channel_sender.send(event).await;
                let _ = rx.await;
                vec![self.reply_effect(uid, &format!("Cleared bans on \x02{}\x02.", channel_name))]
            }
            "OPS" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let event = crate::state::actor::ChannelEvent::Clear {
                    sender_uid: "ChanServ".to_string(),
                    sender_prefix,
                    target: crate::state::actor::ClearTarget::Ops,
                    nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                    reply_tx: tx,
                };
                let _ = channel_sender.send(event).await;
                let _ = rx.await;
                vec![self.reply_effect(uid, &format!("Deopped all users on \x02{}\x02.", channel_name))]
            }
            "VOICES" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let event = crate::state::actor::ChannelEvent::Clear {
                    sender_uid: "ChanServ".to_string(),
                    sender_prefix,
                    target: crate::state::actor::ClearTarget::Voices,
                    nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                    reply_tx: tx,
                };
                let _ = channel_sender.send(event).await;
                let _ = rx.await;
                vec![self.reply_effect(uid, &format!("Devoiced all users on \x02{}\x02.", channel_name))]
            }
            _ => {
                self.error_reply(
                    uid,
                    &format!(
                        "Unknown CLEAR subcommand: \x02{}\x02. Use: CLEAR #channel [USERS|MODES|BANS|OPS|VOICES] [reason]",
                        subcommand
                    ),
                )
            }
        }
    }
}
