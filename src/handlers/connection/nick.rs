//! NICK command handler for connection registration.
//!
//! # RFC 2812 §3.1.2 - Nick message
//!
//! Used to give user a nickname or change the existing one.
//!
//! **Specification:** [RFC 2812 §3.1.2](https://datatracker.ietf.org/doc/html/rfc2812#section-3.1.2)
//!
//! **Compliance:** 11/11 irctest pass
//!
//! ## Syntax
//! ```text
//! NICK <nickname>
//! ```
//!
//! ## Behavior
//! - Can be used before or after registration
//! - Validates nickname format (length, allowed characters)
//! - Atomically reserves nickname to prevent race conditions
//! - Enforces +N (no nick change) channel mode for registered users
//! - Notifies MONITOR watchers when nickname changes

use super::super::{
    Context, HandlerError, HandlerResult, UniversalHandler, notify_monitors_offline,
    notify_monitors_online,
};
use crate::handlers::util::helpers::fanout::broadcast_to_account;
use crate::state::{SessionState, session::SaslAccess};
use async_trait::async_trait;
use dashmap::mapref::entry::Entry;
use slirc_proto::{Command, Message, MessageRef, NickExt, Prefix, Response, irc_to_lower};
use std::time::{Duration, Instant};
use tracing::{debug, info};

const DEFAULT_NICK_MAX_LEN: usize = 30;

#[allow(clippy::result_large_err)]
fn parse_nick_params<'a>(msg: &MessageRef<'a>) -> Result<&'a str, HandlerError> {
    let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
    if nick.is_empty() {
        return Err(HandlerError::NeedMoreParams);
    }
    Ok(nick)
}

#[allow(clippy::result_large_err)]
fn validate_nick(nick: &str) -> Result<(), HandlerError> {
    if !nick.is_valid_nick() {
        return Err(HandlerError::ErroneousNickname(nick.to_string()));
    }
    Ok(())
}

fn is_special(c: char) -> bool {
    matches!(c, '[' | ']' | '\\' | '`' | '_' | '^' | '{' | '|' | '}')
}

fn is_valid_nick_precis(nick: &str) -> bool {
    if nick.is_empty() {
        return false;
    }

    // Keep the same limit we advertise in ISUPPORT (NICKLEN=30).
    // For now we treat this as a byte limit, matching existing RFC1459 validation.
    if nick.len() > DEFAULT_NICK_MAX_LEN {
        return false;
    }

    let mut chars = nick.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    // Under PRECIS, allow Unicode letters as the first character,
    // plus RFC special characters.
    if !(first.is_alphabetic() || is_special(first)) {
        return false;
    }

    // Remaining characters: Unicode letters/digits, RFC specials, or hyphen.
    chars.all(|c| c.is_alphanumeric() || is_special(c) || c == '-')
}

/// Check if two nicks are confusable (one simplifies to the other via Unicode confusables).
fn are_nicks_confusable(nick1: &str, nick2: &str) -> bool {
    use confusables::Confusable;
    // Check if nick1 simplifies to nick2 OR nick2 simplifies to nick1
    nick1.is_confusable_with(nick2) || nick2.is_confusable_with(nick1)
}

pub struct NickHandler;

#[async_trait]
impl<S: SessionState + SaslAccess> UniversalHandler<S> for NickHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // NICK <nickname>
        let nick = parse_nick_params(msg)?;

        match ctx.matrix.config.server.casemapping {
            crate::config::Casemapping::Rfc1459 => validate_nick(nick)?,
            crate::config::Casemapping::Precis => {
                if !is_valid_nick_precis(nick) {
                    return Err(HandlerError::ErroneousNickname(nick.to_string()));
                }
            }
        }

        let nick_lower = irc_to_lower(nick);

        // Check if nick is exactly the same (no-op) - return silently
        if ctx.state.nick().is_some_and(|old| old == nick) {
            return Ok(());
        }

        // Check for confusables under PRECIS casemapping
        if ctx.matrix.config.server.casemapping == crate::config::Casemapping::Precis {
            // Check against all registered nicks for confusables
            for entry in ctx.matrix.user_manager.nicks.iter() {
                let _registered_nick_lower = entry.key();
                let registered_uids = entry.value();

                // Skip if same UID (allow case-only changes)
                if registered_uids.contains(&ctx.uid.to_string()) {
                    continue;
                }

                // Get the actual nick from the first UID
                if let Some(first_uid) = registered_uids.first()
                    && let Some(user_arc) = ctx.matrix.user_manager.users.get(first_uid)
                {
                    let user = user_arc.read().await;
                    // If nicks are confusable, reject
                    if are_nicks_confusable(nick, &user.nick) {
                        return Err(HandlerError::NicknameInUse(nick.to_string()));
                    }
                }
            }

            // Also check against all registered nicks in the database
            // But allow confusable nicks that the current user owns
            match ctx.db.accounts().get_all_registered_nicknames().await {
                Ok(registered_nicks) => {
                    for registered_nick in registered_nicks {
                        // If nicks are confusable, check if user owns it
                        if are_nicks_confusable(nick, &registered_nick) {
                            // Don't reject if it's an exact match (case-insensitive) - user might be resetting
                            if irc_to_lower(nick) == irc_to_lower(&registered_nick) {
                                continue;
                            }
                            // Allow if it's the same as the user's current nick (case-insensitive)
                            if irc_to_lower(&registered_nick)
                                == irc_to_lower(ctx.state.nick().unwrap_or(""))
                            {
                                continue;
                            }
                            // Otherwise reject as confusable
                            return Err(HandlerError::NicknameInUse(nick.to_string()));
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to check registered nicks for confusables: {}", e);
                }
            }
        }

        // Atomically claim nickname (prevents TOCTOU where two clients race between check/insert)
        // For bouncer support: Allow multiple UIDs with same nick if they share the same account
        match ctx.matrix.user_manager.nicks.entry(nick_lower.clone()) {
            Entry::Occupied(mut entry) => {
                let existing_uids = entry.get_mut();

                // Check if this UID is already in the list
                if existing_uids.contains(&ctx.uid.to_string()) {
                    // Same UID, allow case-change or reconnect continuation
                } else {
                    // Different UID - check if accounts match
                    // For pre-registration (before User object exists), check session account
                    // For post-registration, check User object account
                    let current_account =
                        if let Some(user_arc) = ctx.matrix.user_manager.users.get(ctx.uid) {
                            let user = user_arc.read().await;
                            user.account.clone()
                        } else {
                            // Pre-registration: use session account (set by SASL)
                            ctx.state.account().map(String::from)
                        };

                    // Check account of first existing UID (all should have same account)
                    let existing_account = if let Some(first_uid) = existing_uids.first() {
                        if let Some(user_arc) =
                            ctx.matrix.user_manager.users.get(first_uid.as_str())
                        {
                            let user = user_arc.read().await;
                            user.account.clone()
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // Allow if both have accounts and they match
                    // Clone current_account for use in the fallback arm
                    let current_account_for_fallback = current_account.clone();
                    match (current_account, existing_account) {
                        (Some(ref cur_acc), Some(ref exist_acc)) if cur_acc == exist_acc => {
                            // Same account - check if multiclient is allowed for this account
                            let override_opt =
                                ctx.matrix.client_manager.get_multiclient_override(cur_acc);
                            let multiclient_allowed = ctx
                                .matrix
                                .config
                                .multiclient
                                .is_multiclient_enabled(override_opt);

                            if multiclient_allowed {
                                // Allowed - append this UID to the vector
                                existing_uids.push(ctx.uid.to_string());
                            } else {
                                // Multiclient disabled for this account - reject
                                return Err(HandlerError::NicknameInUse(nick.to_string()));
                            }
                        }
                        _ => {
                            // During pre-registration with multiclient enabled, defer validation
                            // The account might not be set yet (SASL may still be in progress)
                            // But if we DO know the account, we must enforce the policy now
                            let multiclient_globally_enabled =
                                ctx.matrix.config.multiclient.enabled;

                            // If we have an account, check specific policy
                            let specific_allowed =
                                if let Some(ref acc) = current_account_for_fallback {
                                    let override_opt =
                                        ctx.matrix.client_manager.get_multiclient_override(acc);
                                    ctx.matrix
                                        .config
                                        .multiclient
                                        .is_multiclient_enabled(override_opt)
                                } else {
                                    // No account yet, fall back to global setting
                                    multiclient_globally_enabled
                                };

                            if !ctx.state.is_registered() && specific_allowed {
                                // Allow the nick collision temporarily
                                // Final validation happens at registration completion
                                debug!(
                                    nick = %nick,
                                    uid = %ctx.uid,
                                    "Allowing pre-registration nick collision for multiclient (will validate at registration)"
                                );
                                existing_uids.push(ctx.uid.to_string());
                            } else {
                                // Either no matching accounts or multiclient disabled - reject
                                return Err(HandlerError::NicknameInUse(nick.to_string()));
                            }
                        }
                    }
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(vec![ctx.uid.to_string()]);
            }
        }

        // Check +N (no nick change) on any channel the user is in
        // Only applies to registered (connected) users changing their nick
        if ctx.state.is_registered()
            && let Some(user_arc) = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone())
        {
            let user = user_arc.read().await;
            for channel_lower in &user.channels {
                let channel_sender = ctx
                    .matrix
                    .channel_manager
                    .channels
                    .get(channel_lower)
                    .map(|c| c.value().clone());
                if let Some(channel_sender) = channel_sender {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::GetInfo {
                            requester_uid: Some(ctx.uid.to_string()),
                            reply_tx: tx,
                        })
                        .await;

                    if let Ok(info) = rx.await
                        && info
                            .modes
                            .contains(&crate::state::actor::ChannelMode::NoNickChange)
                    {
                        let reply =
                            Response::err_nonickchange(ctx.state.nick_or_star(), nick, &info.name)
                                .with_prefix(ctx.server_prefix());
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }
            }
        }

        // Save old nick for NICK change notification (before removing from index)
        let old_nick_for_change = if ctx.state.is_registered() {
            ctx.state.nick().map(|s| s.to_string())
        } else {
            None
        };

        // Check if this is a case-only change (qux -> QUX)
        let is_case_only_change = ctx
            .state
            .nick()
            .map(|old| irc_to_lower(old) == nick_lower)
            .unwrap_or(false);

        // Remove old nick from index if changing
        if let Some(old_nick) = ctx.state.nick() {
            let old_nick_lower = irc_to_lower(old_nick);

            // Only notify MONITOR watchers if the lowercase nick is changing
            // (not for case-only changes like qux -> QUX)
            if ctx.state.is_registered() && !is_case_only_change {
                notify_monitors_offline(ctx.matrix, old_nick).await;
            }

            // If the lowercase nick is changing, remove the old mapping (case-only changes keep mapping)
            if old_nick_lower != nick_lower
                && let Some(mut old_vec) = ctx.matrix.user_manager.nicks.get_mut(&old_nick_lower)
            {
                old_vec.retain(|u| u != ctx.uid);
                if old_vec.is_empty() {
                    drop(old_vec);
                    ctx.matrix.user_manager.nicks.remove(&old_nick_lower);
                }
            }
            // Clear any enforcement timer for old nick
            ctx.matrix.user_manager.enforce_timers.remove(ctx.uid);
        }

        // Register new nick (only if not already present from the entry() logic above)
        // Note: entry() above handles insertion, so we only need to handle case where
        // we're doing a case-only change or the nick wasn't in the map yet
        // Actually, entry() already handled it, so we just update the session state
        ctx.state.set_nick(nick.to_string());

        // Send NICK change message for registered users
        if let Some(old_nick) = old_nick_for_change {
            // Get user info for the prefix and channels
            let (nick_msg, user_channels) = if let Some(user_arc) = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone())
            {
                let user = user_arc.read().await;
                let msg = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
                        old_nick.clone(),
                        user.user.clone(),
                        user.visible_host.clone(),
                    )),
                    command: Command::NICK(nick.to_string()),
                };
                let channels = user.channels.clone();
                (msg, channels)
            } else {
                // Fallback without full user info
                let msg = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
                        old_nick.clone(),
                        "user".to_string(),
                        "host".to_string(),
                    )),
                    command: Command::NICK(nick.to_string()),
                };
                (msg, std::collections::HashSet::new())
            };

            // Send to the user themselves with label (IRCv3 labeled-response)
            let labeled_nick_msg = super::super::with_label(nick_msg.clone(), ctx.label.as_deref());
            ctx.sender.send(labeled_nick_msg).await?;

            // Broadcast to all channels the user is in (including case-only changes)
            for channel_lower in &user_channels {
                ctx.matrix
                    .channel_manager
                    .broadcast_to_channel(channel_lower, nick_msg.clone(), Some(ctx.uid))
                    .await;

                // Update the channel actor's user_nicks map
                let channel_sender = ctx
                    .matrix
                    .channel_manager
                    .channels
                    .get(channel_lower)
                    .map(|c| c.value().clone());
                if let Some(channel_sender) = channel_sender {
                    let _ = channel_sender
                        .send(crate::state::actor::ChannelEvent::NickChange {
                            uid: ctx.uid.to_string(),
                            new_nick: nick.to_string(),
                        })
                        .await;
                }
            }

            if ctx.matrix.config.multiclient.enabled {
                broadcast_to_account(ctx, nick_msg.clone(), true).await?;
            }

            // Also update the User state with the new nick
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            let account = if let Some(user_arc) = user_arc {
                let mut user = user_arc.write().await;
                user.nick = nick.to_string();
                user.account.clone()
            } else {
                None
            };

            if ctx.matrix.config.multiclient.enabled
                && let Some(account) = account
            {
                ctx.matrix.client_manager.update_nick(&account, nick).await;
            }

            // Notify observer of user update (Innovation 2)
            ctx.matrix.user_manager.notify_observer(ctx.uid, None).await;
        }

        // Notify MONITOR watchers that new nick is online (only for already-registered users)
        // Skip notification for case-only changes (already computed above)
        if ctx.state.is_registered() && !is_case_only_change {
            // Get user info for the hostmask
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                notify_monitors_online(ctx.matrix, nick, &user.user, &user.visible_host).await;
            }
        }

        debug!(nick = %nick, uid = %ctx.uid, "Nick set");

        // Check if nick enforcement should be started
        // Only if user is not already identified to an account
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        let is_identified = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.modes.registered
        } else {
            false
        };

        if !is_identified {
            // Check if this nick is registered with ENFORCE enabled
            if let Ok(Some(account)) = ctx.db.accounts().find_by_nickname(nick).await
                && account.enforce
            {
                // Start 60 second timer
                let deadline = Instant::now() + Duration::from_secs(60);
                ctx.matrix
                    .user_manager
                    .enforce_timers
                    .insert(ctx.uid.to_string(), deadline);

                // Notify user
                let notice = Message {
                    tags: None,
                    prefix: Some(Prefix::new(
                        "NickServ".to_string(),
                        "NickServ".to_string(),
                        "services.".to_string(),
                    )),
                    command: Command::NOTICE(
                        nick.to_string(),
                        "This nickname is registered. Please identify via \x02/msg NickServ IDENTIFY <password>\x02 within 60 seconds.".to_string(),
                    ),
                };
                let _ = ctx.sender.send(notice).await;
                info!(nick = %nick, uid = %ctx.uid, "Nick enforcement timer started");
            }
        }

        // Note: can_register() is only relevant for UnregisteredState.
        // For RegisteredState, we're already registered so this is a no-op.
        // The connection loop handles the registration transition.

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::MessageRef;

    #[test]
    fn test_parse_nick_params_valid() {
        let msg = MessageRef::parse("NICK valid_nick").unwrap();
        let nick = parse_nick_params(&msg).unwrap();
        assert_eq!(nick, "valid_nick");
    }

    #[test]
    fn test_parse_nick_params_missing() {
        let msg = MessageRef::parse("NICK").unwrap();
        let err = parse_nick_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_parse_nick_params_empty() {
        let msg = MessageRef::parse("NICK :").unwrap();
        let err = parse_nick_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_validate_nick_valid() {
        assert!(validate_nick("valid").is_ok());
        assert!(validate_nick("Valid123").is_ok());
        assert!(validate_nick("[valid]").is_ok());
    }

    #[test]
    fn test_validate_nick_invalid() {
        let err = validate_nick("1invalid").unwrap_err();
        assert!(matches!(err, HandlerError::ErroneousNickname(_)));

        let err = validate_nick("invalid space").unwrap_err();
        assert!(matches!(err, HandlerError::ErroneousNickname(_)));

        let err = validate_nick("").unwrap_err();
        assert!(matches!(err, HandlerError::ErroneousNickname(_)));
    }

    #[test]
    fn test_validate_nick_precis_unicode() {
        assert!(is_valid_nick_precis("Işıl"));
        assert!(!is_valid_nick_precis("1Işıl"));
        assert!(!is_valid_nick_precis("Işıl space"));
    }
}
