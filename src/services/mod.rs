//! IRC services module.
//!
//! Provides virtual services like NickServ and ChanServ.

pub mod chanserv;
pub mod enforce;
pub mod nickserv;

use crate::state::Matrix;
use slirc_proto::{ChannelMode, Command, Message, Mode, Prefix, irc_to_lower};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// Unified effect type returned by all service commands.
///
/// Services produce effects; callers (handlers) apply them to Matrix state.
/// This decouples service logic from state mutation, improving testability
/// and preparing for server-linking (effects can be forwarded).
#[derive(Debug, Clone)]
pub enum ServiceEffect {
    /// Send a message to a specific user (e.g., NOTICE reply).
    Reply {
        /// Target UID (currently unused - replies go to sender directly).
        /// TODO: Use for routing when sender != target (e.g., admin commands)
        #[allow(dead_code)]
        target_uid: String,
        msg: Message,
    },

    /// Set user's account and +r mode (successful IDENTIFY/REGISTER).
    AccountIdentify { target_uid: String, account: String },

    /// Clear user's account and -r mode (DROP).
    AccountClear { target_uid: String },

    /// Clear enforcement timer for a user (cancels pending nick enforcement).
    ClearEnforceTimer { target_uid: String },

    /// Disconnect a user (GHOST, AKICK, KILL).
    Kill {
        target_uid: String,
        killer: String,
        reason: String,
    },

    /// Kick a user from a channel (ChanServ CLEAR, AKICK enforcement).
    Kick {
        channel: String,
        target_uid: String,
        kicker: String,
        reason: String,
    },

    /// Apply channel mode change (ChanServ OP/DEOP/VOICE).
    ChannelMode {
        channel: String,
        target_uid: String,
        mode_char: char,
        adding: bool,
    },

    /// Force nick change (enforcement).
    ForceNick {
        target_uid: String,
        old_nick: String,
        new_nick: String,
    },

    /// Broadcast account change to all shared channels (account-notify capability).
    /// Sends `:old_prefix ACCOUNT new_account` to channel members with account-notify.
    /// If new_account is "*", user logged out.
    /// TODO: Use from NickServ IDENTIFY/LOGOUT when account-notify is fully implemented.
    #[allow(dead_code)]
    BroadcastAccount {
        target_uid: String,
        /// Account name, or "*" for logout.
        new_account: String,
    },

    /// Broadcast host change to all shared channels (chghost capability).
    /// Sends `:old_prefix CHGHOST new_user new_host` to channel members with chghost.
    /// TODO: Use from HostServ or vhost changes when chghost is fully implemented.
    #[allow(dead_code)]
    BroadcastChghost {
        target_uid: String,
        new_user: String,
        new_host: String,
    },
}

use crate::db::Database;

/// Unified service message router.
///
/// Routes PRIVMSG/SQUERY to NickServ or ChanServ based on target.
/// Returns true if the message was handled by a service.
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

    match target_lower.as_str() {
        "nickserv" | "ns" => {
            let ns = nickserv::NickServ::new(db.clone());
            let effects = ns.handle(matrix, uid, nick, text).await;
            apply_effects(matrix, nick, sender, effects).await;
            true
        }
        "chanserv" | "cs" => {
            let cs = chanserv::ChanServ::new(db.clone());
            let effects = cs.handle(matrix, uid, nick, text).await;
            apply_effects(matrix, nick, sender, effects).await;
            true
        }
        _ => false,
    }
}

/// Apply a list of service effects sequentially.
///
/// Convenience wrapper for applying multiple effects in one go.
pub async fn apply_effects(
    matrix: &Arc<Matrix>,
    nick: &str,
    sender: &mpsc::Sender<Message>,
    effects: Vec<ServiceEffect>,
) {
    for effect in effects {
        apply_effect(matrix, nick, sender, effect).await;
    }
}

/// Apply a single service effect to Matrix state.
///
/// This is the centralized effect application logic. All services return effects,
/// and callers use this function to apply them consistently.
pub async fn apply_effect(
    matrix: &Arc<Matrix>,
    nick: &str,
    sender: &mpsc::Sender<Message>,
    effect: ServiceEffect,
) {
    match effect {
        ServiceEffect::Reply {
            target_uid: _,
            mut msg,
        } => {
            // Set the target nick for the NOTICE
            if let Command::NOTICE(_, text) = &msg.command {
                msg.command = Command::NOTICE(nick.to_string(), text.clone());
            }
            let _ = sender.send(msg).await;
        }

        ServiceEffect::AccountIdentify {
            target_uid,
            account,
        } => {
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
                    vec![slirc_proto::Mode::Plus(
                        slirc_proto::UserMode::Registered,
                        None,
                    )],
                ),
            };

            // Broadcast ACCOUNT message for account-notify capability (IRCv3.1)
            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT(account.clone()),
            };

            for channel_name in &channels {
                matrix
                    .broadcast_to_channel(channel_name, mode_msg.clone(), None)
                    .await;
                matrix
                    .broadcast_to_channel(channel_name, account_msg.clone(), None)
                    .await;
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(mode_msg).await;
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, account = %account, "User identified to account");
        }

        ServiceEffect::AccountClear { target_uid } => {
            // Get user info for MODE broadcast
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

            // Clear +r mode and account on user
            if let Some(user_ref) = matrix.users.get(&target_uid) {
                let mut user = user_ref.write().await;
                user.modes.registered = false;
                user.account = None;
            }

            // Broadcast MODE -r to all channels the user is in
            let mode_msg = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                command: Command::UserMODE(
                    nick.clone(),
                    vec![slirc_proto::Mode::Minus(
                        slirc_proto::UserMode::Registered,
                        None,
                    )],
                ),
            };

            // Broadcast ACCOUNT * message (account unset)
            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT("*".to_string()),
            };

            for channel_name in &channels {
                matrix
                    .broadcast_to_channel(channel_name, mode_msg.clone(), None)
                    .await;
                matrix
                    .broadcast_to_channel(channel_name, account_msg.clone(), None)
                    .await;
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(mode_msg).await;
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, "User account cleared");
        }

        ServiceEffect::ClearEnforceTimer { target_uid } => {
            matrix.enforce_timers.remove(&target_uid);
            info!(uid = %target_uid, "Enforcement timer cleared");
        }

        ServiceEffect::Kill {
            target_uid,
            killer,
            reason,
        } => {
            // Disconnect the user
            matrix
                .disconnect_user(&target_uid, &format!("Killed by {}: {}", killer, reason))
                .await;

            info!(uid = %target_uid, killer = %killer, reason = %reason, "User killed by service");
        }

        ServiceEffect::ChannelMode {
            channel,
            target_uid,
            mode_char,
            adding,
        } => {
            // Get target nick for MODE message
            let target_nick = if let Some(user_ref) = matrix.users.get(&target_uid) {
                user_ref.read().await.nick.clone()
            } else {
                return;
            };

            // Get canonical channel name
            let channel_lower = irc_to_lower(&channel);
            let canonical_name = if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                channel_ref.read().await.name.clone()
            } else {
                return;
            };

            // Apply mode change to channel member
            if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                let mut channel_guard = channel_ref.write().await;
                if let Some(member) = channel_guard.members.get_mut(&target_uid) {
                    match mode_char {
                        'o' => member.op = adding,
                        'v' => member.voice = adding,
                        _ => {}
                    }
                }
            }

            // Build typed MODE message from ChanServ
            let channel_mode = match mode_char {
                'o' => ChannelMode::Oper,
                'v' => ChannelMode::Voice,
                'h' => ChannelMode::Halfop,
                c => ChannelMode::Unknown(c),
            };

            let mode_change = if adding {
                Mode::plus(channel_mode, Some(&target_nick))
            } else {
                Mode::minus(channel_mode, Some(&target_nick))
            };

            let mode_str = mode_change.flag();

            let mode_msg = Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    "ChanServ".to_string(),
                    "ChanServ".to_string(),
                    "services.".to_string(),
                )),
                command: Command::ChannelMODE(canonical_name.clone(), vec![mode_change]),
            };

            // Broadcast MODE change to channel members
            matrix.broadcast_to_channel(&channel, mode_msg, None).await;

            info!(channel = %canonical_name, target = %target_nick, mode = %mode_str, "ChanServ mode change");
        }

        ServiceEffect::Kick {
            channel,
            target_uid,
            kicker,
            reason,
        } => {
            // Get target nick for KICK message
            let target_nick = if let Some(user_ref) = matrix.users.get(&target_uid) {
                user_ref.read().await.nick.clone()
            } else {
                return;
            };

            // Get canonical channel name
            let channel_lower = irc_to_lower(&channel);
            let canonical_name = if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                channel_ref.read().await.name.clone()
            } else {
                return;
            };

            // Build KICK message from ChanServ
            let kick_msg = Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    kicker.clone(),
                    kicker.clone(),
                    "services.".to_string(),
                )),
                command: Command::KICK(canonical_name.clone(), target_nick.clone(), Some(reason.clone())),
            };

            // Broadcast KICK to channel members
            matrix
                .broadcast_to_channel(&channel_lower, kick_msg, None)
                .await;

            // Remove user from channel state
            if let Some(channel_ref) = matrix.channels.get(&channel_lower) {
                let mut channel_guard = channel_ref.write().await;
                channel_guard.members.remove(&target_uid);
            }

            // Remove channel from user's channel list
            if let Some(user_ref) = matrix.users.get(&target_uid) {
                let mut user_guard = user_ref.write().await;
                user_guard.channels.remove(&channel_lower);
            }

            info!(channel = %canonical_name, target = %target_nick, kicker = %kicker, reason = %reason, "User kicked by service");
        }

        ServiceEffect::ForceNick {
            target_uid,
            old_nick,
            new_nick,
        } => {
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
                matrix
                    .broadcast_to_channel(channel_name, nick_msg.clone(), None)
                    .await;
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(nick_msg).await;
            }

            info!(uid = %target_uid, old = %old_nick, new = %new_nick, "Forced nick change");
        }

        ServiceEffect::BroadcastAccount {
            target_uid,
            new_account,
        } => {
            // Get user info for ACCOUNT broadcast
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

            // Build ACCOUNT message: :nick!user@host ACCOUNT accountname
            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT(new_account.clone()),
            };

            // Broadcast to all shared channels (only to clients with account-notify)
            for channel_name in &channels {
                matrix
                    .broadcast_to_channel_with_cap(
                        channel_name,
                        account_msg.clone(),
                        None,
                        Some("account-notify"),
                        None, // No fallback - clients without cap get nothing
                    )
                    .await;
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, account = %new_account, "Broadcast account change");
        }

        ServiceEffect::BroadcastChghost {
            target_uid,
            new_user,
            new_host,
        } => {
            // Get user info for CHGHOST broadcast BEFORE updating
            let (nick, old_user, old_host, channels) = {
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

            // Build CHGHOST message: :nick!old_user@old_host CHGHOST new_user new_host
            let chghost_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &old_user, &old_host)),
                command: Command::CHGHOST(new_user.clone(), new_host.clone()),
            };

            // Broadcast to all shared channels (only to clients with chghost)
            for channel_name in &channels {
                matrix
                    .broadcast_to_channel_with_cap(
                        channel_name,
                        chghost_msg.clone(),
                        None,
                        Some("chghost"),
                        None, // No fallback - clients without cap get nothing
                    )
                    .await;
            }

            // Update the user's user and host fields
            if let Some(user_ref) = matrix.users.get(&target_uid) {
                let mut user = user_ref.write().await;
                user.user = new_user.clone();
                user.host = new_host.clone();
            }

            // Also send to the user themselves
            if let Some(sender) = matrix.senders.get(&target_uid) {
                let _ = sender.send(chghost_msg).await;
            }

            info!(uid = %target_uid, new_user = %new_user, new_host = %new_host, "Broadcast host change");
        }
    }
}
