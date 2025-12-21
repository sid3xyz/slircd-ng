//! IRC services module.
//!
//! Provides virtual services like NickServ and ChanServ.

pub mod base;
pub mod chanserv;
pub mod enforce;
pub mod nickserv;
pub mod traits;

pub use traits::Service;

use crate::{handlers::ResponseMiddleware, state::Matrix};
use slirc_proto::{ChannelMode, Command, Message, Mode, Prefix, irc_to_lower};
use std::sync::Arc;
use tracing::info;

/// Unified effect type returned by all service commands.
///
/// Services produce effects; callers (handlers) apply them to Matrix state.
/// This decouples service logic from state mutation, improving testability
/// and preparing for server-linking (effects can be forwarded).
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ServiceEffect {
    /// Send a message to a specific user (e.g., NOTICE reply).
    Reply {
        /// Target UID to route the reply to.
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
    BroadcastAccount {
        target_uid: String,
        /// Account name, or "*" for logout.
        new_account: String,
    },
}

/// Unified service message router.
///
/// Routes PRIVMSG/SQUERY to NickServ or ChanServ based on target.
/// Returns true if the message was handled by a service.
///
/// Services are singletons stored in Matrix, created once at server startup.
pub async fn route_service_message(
    matrix: &Arc<Matrix>,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &ResponseMiddleware<'_>,
) -> bool {
    let target_lower = irc_to_lower(target);

    // Check core services first
    if target_lower == "nickserv" || target_lower == "ns" {
        let effects = matrix
            .service_manager
            .nickserv
            .handle_command(matrix, uid, nick, text)
            .await;
        apply_effects(matrix, nick, sender, effects).await;
        return true;
    }

    if target_lower == "chanserv" || target_lower == "cs" {
        let effects = matrix
            .service_manager
            .chanserv
            .handle_command(matrix, uid, nick, text)
            .await;
        apply_effects(matrix, nick, sender, effects).await;
        return true;
    }

    // Check extra services
    // We iterate because we need to check aliases too.
    // Optimization: We could build a lookup map in Matrix::new, but for now iteration is fine
    // as the number of services is small.
    for service in matrix.service_manager.extra_services.values() {
        if irc_to_lower(service.name()) == target_lower
            || service
                .aliases()
                .iter()
                .any(|a| irc_to_lower(a) == target_lower)
        {
            let effects = service.handle(matrix, uid, nick, text).await;
            apply_effects(matrix, nick, sender, effects).await;
            return true;
        }
    }

    false
}

/// Apply a list of service effects sequentially.
///
/// Convenience wrapper for applying multiple effects in one go.
pub async fn apply_effects(
    matrix: &Arc<Matrix>,
    nick: &str,
    sender: &ResponseMiddleware<'_>,
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
    _nick: &str,
    _sender: &ResponseMiddleware<'_>,
    effect: ServiceEffect,
) {
    match effect {
        ServiceEffect::Reply {
            target_uid,
            mut msg,
        } => {
            // 1. Resolve the target nickname for the NOTICE command
            let target_nick = if let Some(user_arc) = matrix.user_manager.users.get(&target_uid) {
                user_arc.read().await.nick.clone()
            } else {
                // User not found (disconnected?)
                return;
            };

            // 2. Route the message to the target's sender channel
            if let Some(target_tx) = matrix.user_manager.senders.get(&target_uid) {
                // Update the NOTICE command with the correct target nick
                // Move text out of old command to avoid clone
                if let Command::NOTICE(_, text) = msg.command {
                    msg.command = Command::NOTICE(target_nick, text);
                }
                let _ = target_tx.send(msg).await;
            }
        }

        ServiceEffect::AccountIdentify {
            target_uid,
            account,
        } => {
            // Get user info for MODE broadcast before we modify the user
            let nick = {
                let user_arc = matrix
                    .user_manager
                    .users
                    .get(&target_uid)
                    .map(|u| u.clone());
                if let Some(user_arc) = user_arc {
                    let user = user_arc.read().await;
                    user.nick.clone()
                } else {
                    return;
                }
            };

            info!(uid = %target_uid, account = %account, "User identified to account");

            // Set +r mode and account on user
            let user_arc = matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let mut user = user_arc.write().await;
                user.modes.registered = true;
                user.account = Some(account);
            }

            // Clear any nick enforcement timer
            matrix.user_manager.enforce_timers.remove(&target_uid);

            // Send MODE +r directly to the user (user modes are not broadcast)
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

            let sender = matrix
                .user_manager
                .senders
                .get(&target_uid)
                .map(|s| s.clone());
            if let Some(sender) = sender {
                let _ = sender.send(mode_msg).await;
            }
        }

        ServiceEffect::AccountClear { target_uid } => {
            // Get user info for MODE broadcast
            let nick = {
                let user_arc = matrix
                    .user_manager
                    .users
                    .get(&target_uid)
                    .map(|u| u.clone());
                if let Some(user_arc) = user_arc {
                    let user = user_arc.read().await;
                    user.nick.clone()
                } else {
                    return;
                }
            };

            // Clear +r mode and account on user
            let user_arc = matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let mut user = user_arc.write().await;
                user.modes.registered = false;
                user.account = None;
            }

            // Send MODE -r directly to the user (user modes are not broadcast)
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

            let sender = matrix
                .user_manager
                .senders
                .get(&target_uid)
                .map(|s| s.clone());
            if let Some(sender) = sender {
                let _ = sender.send(mode_msg).await;
            }

            info!(uid = %target_uid, "User account cleared");
        }

        ServiceEffect::ClearEnforceTimer { target_uid } => {
            matrix.user_manager.enforce_timers.remove(&target_uid);
            info!(uid = %target_uid, "Enforcement timer cleared");
        }

        ServiceEffect::Kill {
            target_uid,
            killer,
            reason,
        } => {
            // Disconnect the user
            let quit_reason = format!("Killed by {}: {}", killer, reason);
            matrix.disconnect_user(&target_uid, &quit_reason).await;

            info!(uid = %target_uid, killer = %killer, reason = %reason, "User killed by service");
        }

        ServiceEffect::ChannelMode {
            channel,
            target_uid,
            mode_char,
            adding,
        } => {
            // Get target nick for MODE message
            let user_arc = matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            let target_nick = if let Some(user_arc) = user_arc {
                user_arc.read().await.nick.clone()
            } else {
                return;
            };

            let channel_lower = irc_to_lower(&channel);
            let channel_sender =
                if let Some(c) = matrix.channel_manager.channels.get(&channel_lower) {
                    c.clone()
                } else {
                    return;
                };

            // Build typed MODE message from ChanServ
            let channel_mode = match mode_char {
                'o' => ChannelMode::Oper,
                'v' => ChannelMode::Voice,
                'h' => ChannelMode::Halfop,
                c => ChannelMode::Unknown(c),
            };

            let mode_obj = if adding {
                Mode::plus(channel_mode, Some(&target_nick))
            } else {
                Mode::minus(channel_mode, Some(&target_nick))
            };

            let mut target_uids = std::collections::HashMap::with_capacity(1);
            target_uids.insert(target_nick.clone(), target_uid.clone());

            let sender_prefix = Prefix::new(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            );

            let (tx, rx) = tokio::sync::oneshot::channel();
            let event = crate::state::actor::ChannelEvent::ApplyModes {
                params: crate::state::actor::ModeParams {
                    sender_uid: "ChanServ".to_string(),
                    sender_prefix,
                    modes: vec![mode_obj],
                    target_uids,
                    force: true,
                },
                reply_tx: tx,
            };

            let _ = channel_sender.send(event).await;
            let _ = rx.await;
        }

        ServiceEffect::Kick {
            channel,
            target_uid,
            kicker,
            reason,
        } => {
            // Get target nick for KICK message
            let user_arc = matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            let target_nick = if let Some(user_arc) = user_arc {
                user_arc.read().await.nick.clone()
            } else {
                return;
            };

            let channel_lower = irc_to_lower(&channel);
            let channel_sender =
                if let Some(c) = matrix.channel_manager.channels.get(&channel_lower) {
                    c.clone()
                } else {
                    return;
                };

            let sender_prefix =
                Prefix::new(kicker.clone(), kicker.clone(), "services.".to_string());

            let (tx, rx) = tokio::sync::oneshot::channel();
            let event = crate::state::actor::ChannelEvent::Kick {
                params: crate::state::actor::KickParams {
                    sender_uid: kicker.clone(),
                    sender_prefix,
                    target_uid: target_uid.clone(),
                    target_nick: target_nick.clone(),
                    reason: reason.clone(),
                    force: true,
                    cap: None,
                },
                reply_tx: tx,
            };

            if let Ok(_) = channel_sender.send(event).await
                && let Ok(Ok(())) = rx.await
            {
                // Success
                // Remove channel from user's channel list
                let user_arc = matrix
                    .user_manager
                    .users
                    .get(&target_uid)
                    .map(|u| u.clone());
                if let Some(user_arc) = user_arc {
                    let mut user_guard = user_arc.write().await;
                    user_guard.channels.remove(&channel_lower);
                }
            }

            info!(channel = %channel, target = %target_nick, kicker = %kicker, reason = %reason, "User kicked by service");
        }

        ServiceEffect::ForceNick {
            target_uid,
            old_nick,
            new_nick,
        } => {
            // Get user info for NICK message before we modify
            let (username, hostname, channels) = {
                let user_arc = matrix
                    .user_manager
                    .users
                    .get(&target_uid)
                    .map(|u| u.clone());
                if let Some(user_arc) = user_arc {
                    let user = user_arc.read().await;
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

            matrix.user_manager.nicks.remove(&old_nick_lower);
            matrix
                .user_manager
                .nicks
                .insert(new_nick_lower, target_uid.clone());

            let user_arc = matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.clone());
            if let Some(user_arc) = user_arc {
                let mut user = user_arc.write().await;
                user.nick = new_nick.clone();
            }

            // Build NICK message
            let nick_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(old_nick.clone(), username, hostname)),
                command: Command::NICK(new_nick.clone()),
            };

            // Broadcast NICK change to all shared channels
            for channel_name in &channels {
                matrix
                    .channel_manager
                    .broadcast_to_channel(channel_name, nick_msg.clone(), None)
                    .await;
            }

            // Also send to the user themselves
            let sender = matrix
                .user_manager
                .senders
                .get(&target_uid)
                .map(|s| s.clone());
            if let Some(sender) = sender {
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
                let user_arc = matrix
                    .user_manager
                    .users
                    .get(&target_uid)
                    .map(|u| u.clone());
                if let Some(user_arc) = user_arc {
                    let user = user_arc.read().await;
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
                    .channel_manager
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
            let sender = matrix
                .user_manager
                .senders
                .get(&target_uid)
                .map(|s| s.clone());
            if let Some(sender) = sender {
                let _ = sender.send(account_msg).await;
            }

            info!(uid = %target_uid, account = %new_account, "Broadcast account change");
        }
    }
}
