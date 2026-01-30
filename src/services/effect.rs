use crate::handlers::{ResponseMiddleware, notify_extended_monitor_watchers};
use crate::state::Matrix;
use crate::state::dashmap_ext::DashMapExt;
use crate::state::observer::StateObserver;
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
    AccountIdentify {
        target_uid: String,
        account: String,
        account_id: Option<i64>,
    },

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

/// Helper: Resolve nick from UID.
async fn resolve_user_nick(matrix: &Arc<Matrix>, uid: &str) -> Option<String> {
    if let Some(user_arc) = matrix.user_manager.users.get_cloned(uid) {
        let user = user_arc.read().await;
        Some(user.nick.clone())
    } else {
        None
    }
}

/// Helper: Route message to user (local or remote).
async fn route_to_user(
    matrix: &Arc<Matrix>,
    target_uid: &str,
    target_nick: &str,
    mut msg: Message,
) {
    // try local sender first
    if let Some(target_tx) = matrix.user_manager.get_first_sender(target_uid) {
        if let Command::NOTICE(_, text) = msg.command {
            msg.command = Command::NOTICE(target_nick.to_string(), text);
        }
        let _ = target_tx.send(Arc::new(msg)).await;
    } else {
        // Remote user - route via S2S
        if let Command::NOTICE(_, text) = msg.command {
            msg.command = Command::NOTICE(target_nick.to_string(), text);
        }
        let _ = matrix
            .sync_manager
            .route_to_remote_user(target_uid, Arc::new(msg))
            .await;
    }
}

/// Apply a list of service effects without a ResponseMiddleware.
///
/// Used for handling remote service requests via S2S where we don't
/// have a local sender connection.
pub async fn apply_effects_no_sender(
    matrix: &Arc<Matrix>,
    nick: &str,
    effects: Vec<ServiceEffect>,
) {
    for effect in effects {
        apply_effect_no_sender(matrix, nick, effect).await;
    }
}

/// Apply a single service effect without a ResponseMiddleware.
pub async fn apply_effect_no_sender(matrix: &Arc<Matrix>, nick: &str, effect: ServiceEffect) {
    // Delegate to the main apply_effect - logic is now unified or handles missing sender gracefully
    // Pass a dummy sender for now, or better: Refactor apply_effect to take Option<&ResponseMiddleware>
    // Since we can't easily change the signature of apply_effect without breaking callers,
    // we will just inline the unified logic here or call a shared private helper.

    // Actually, let's call the shared private implementation
    apply_effect_impl(matrix, nick, None, effect).await;
}

/// Apply a single service effect to Matrix state.
pub async fn apply_effect(
    matrix: &Arc<Matrix>,
    nick: &str,
    sender: &ResponseMiddleware<'_>,
    effect: ServiceEffect,
) {
    apply_effect_impl(matrix, nick, Some(sender), effect).await;
}

/// Shared implementation for effect application.
async fn apply_effect_impl(
    matrix: &Arc<Matrix>,
    _nick: &str,
    _sender: Option<&ResponseMiddleware<'_>>,
    effect: ServiceEffect,
) {
    match effect {
        ServiceEffect::Reply { target_uid, msg } => {
            if let Some(nick) = resolve_user_nick(matrix, &target_uid).await {
                route_to_user(matrix, &target_uid, &nick, msg).await;
            }
        }

        ServiceEffect::AccountIdentify {
            target_uid,
            account,
            account_id,
        } => {
            if let Some(nick) = resolve_user_nick(matrix, &target_uid).await {
                info!(uid = %target_uid, account = %account, "User identified to account");

                // Update user state
                if let Some(user_arc) = matrix.user_manager.users.get_cloned(&target_uid) {
                    let mut user = user_arc.write().await;
                    user.modes.registered = true;
                    user.account = Some(account.clone());
                    user.account_id = account_id;
                }

                // Broadcast to S2S
                matrix
                    .sync_manager
                    .on_account_change(&target_uid, Some(&account), None);

                // Clear enforce timer
                matrix.user_manager.enforce_timers.remove(&target_uid);

                // Send MODE +r to user
                let mode_msg = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                    command: Command::UserMODE(
                        nick,
                        vec![slirc_proto::Mode::Plus(
                            slirc_proto::UserMode::Registered,
                            None,
                        )],
                    ),
                };
                matrix
                    .user_manager
                    .send_to_uid(&target_uid, Arc::new(mode_msg))
                    .await;
            }
        }

        ServiceEffect::AccountClear { target_uid } => {
            if let Some(nick) = resolve_user_nick(matrix, &target_uid).await {
                // Update user state
                if let Some(user_arc) = matrix.user_manager.users.get_cloned(&target_uid) {
                    let mut user = user_arc.write().await;
                    user.modes.registered = false;
                    user.account = None;
                    user.account_id = None;
                }

                // Broadcast to S2S
                matrix
                    .sync_manager
                    .on_account_change(&target_uid, None, None);

                // Send MODE -r to user
                let mode_msg = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(matrix.server_info.name.clone())),
                    command: Command::UserMODE(
                        nick,
                        vec![slirc_proto::Mode::Minus(
                            slirc_proto::UserMode::Registered,
                            None,
                        )],
                    ),
                };
                matrix
                    .user_manager
                    .send_to_uid(&target_uid, Arc::new(mode_msg))
                    .await;

                info!(uid = %target_uid, "User account cleared");
            }
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
            if let Some(target_nick) = resolve_user_nick(matrix, &target_uid).await {
                let channel_lower = irc_to_lower(&channel);
                if let Some(c) = matrix.channel_manager.channels.get(&channel_lower) {
                    let channel_sender = c.value().clone();

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
                    target_uids.insert(target_nick.clone(), vec![target_uid.clone()]);

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
                            nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                        },
                        reply_tx: tx,
                    };

                    let _ = channel_sender.send(event).await;
                    let _ = rx.await;
                }
            }
        }

        ServiceEffect::Kick {
            channel,
            target_uid,
            kicker,
            reason,
        } => {
            if let Some(target_nick) = resolve_user_nick(matrix, &target_uid).await {
                let channel_lower = irc_to_lower(&channel);
                if let Some(c) = matrix.channel_manager.channels.get(&channel_lower) {
                    let channel_sender = c.value().clone();

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
                            nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                        },
                        reply_tx: tx,
                    };

                    if let Ok(_) = channel_sender.send(event).await
                        && let Ok(Ok(())) = rx.await
                    {
                        // Success - update user state
                        if let Some(user_arc) = matrix.user_manager.users.get_cloned(&target_uid) {
                            let mut user = user_arc.write().await;
                            user.channels.remove(&channel_lower);
                        }
                    }

                    info!(channel = %channel, target = %target_nick, kicker = %kicker, reason = %reason, "User kicked by service");
                }
            }
        }

        ServiceEffect::ForceNick {
            target_uid,
            old_nick,
            new_nick,
        } => {
            // Get user info
            let (username, hostname, channels) = {
                let user_arc = matrix.user_manager.users.get_cloned(&target_uid);
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

            if let Some(mut vec) = matrix.user_manager.nicks.get_mut(&old_nick_lower) {
                vec.retain(|u| u != &target_uid);
                if vec.is_empty() {
                    drop(vec);
                    matrix.user_manager.nicks.remove(&old_nick_lower);
                }
            }

            matrix
                .user_manager
                .nicks
                .entry(new_nick_lower)
                .or_insert_with(Vec::new)
                .push(target_uid.clone());

            if let Some(user_arc) = matrix.user_manager.users.get_cloned(&target_uid) {
                let mut user = user_arc.write().await;
                user.nick = new_nick.clone();
            }

            // Build NICK message
            let nick_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(old_nick.clone(), username, hostname)),
                command: Command::NICK(new_nick.clone()),
            };

            // Broadcast
            for channel_name in &channels {
                matrix
                    .channel_manager
                    .broadcast_to_channel(channel_name, nick_msg.clone(), None)
                    .await;
            }

            matrix
                .user_manager
                .send_to_uid(&target_uid, Arc::new(nick_msg))
                .await;
            info!(uid = %target_uid, old = %old_nick, new = %new_nick, "Forced nick change");
        }

        ServiceEffect::BroadcastAccount {
            target_uid,
            new_account,
        } => {
            let (nick, user_str, host, channels) = {
                let user_arc = matrix.user_manager.users.get_cloned(&target_uid);
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

            let account_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(&nick, &user_str, &host)),
                command: Command::ACCOUNT(new_account.clone()),
            };

            for channel_name in &channels {
                matrix
                    .channel_manager
                    .broadcast_to_channel_with_cap(
                        channel_name,
                        account_msg.clone(),
                        None,
                        Some("account-notify"),
                        None,
                    )
                    .await;
            }

            matrix
                .user_manager
                .send_to_uid(&target_uid, Arc::new(account_msg.clone()))
                .await;
            notify_extended_monitor_watchers(matrix, &nick, account_msg, "account-notify").await;

            let account_opt = if new_account == "*" {
                None
            } else {
                Some(new_account.as_str())
            };
            matrix
                .sync_manager
                .on_account_change(&target_uid, account_opt, None);

            info!(uid = %target_uid, account = %new_account, "Broadcast account change");
        }
    }
}
