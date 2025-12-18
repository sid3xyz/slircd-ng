//! Common utilities for message handling.
//!
//! Shared helpers for PRIVMSG, NOTICE, and TAGMSG handlers including shun checking,
//! routing logic, channel validation, and error responses.

use super::super::{Context, HandlerResult, server_reply};
use crate::security::UserContext;
use slirc_proto::ctcp::{Ctcp, CtcpKind};
use slirc_proto::{Command, Message, Response};
use tracing::debug;

// ============================================================================
// Sender Snapshot - Pre-fetched user data to avoid redundant lookups
// ============================================================================

/// Pre-captured sender information to eliminate redundant user lookups.
///
/// InspIRCd pattern: Build complete sender context once at handler entry,
/// then pass by reference to all routing functions.
#[derive(Debug, Clone)]
pub struct SenderSnapshot {
    /// Sender's nickname.
    pub nick: String,
    /// Sender's username (ident).
    pub user: String,
    /// Sender's real hostname.
    pub host: String,
    /// Sender's visible (possibly cloaked) hostname.
    pub visible_host: String,
    /// Sender's realname (GECOS).
    pub realname: String,
    /// Sender's IP address.
    pub ip: String,
    /// Account name if identified.
    pub account: Option<String>,
    /// Whether sender is identified (+r).
    pub is_registered: bool,
    /// Whether sender is an IRC operator.
    pub is_oper: bool,
    /// Whether sender is marked as a bot (+B).
    pub is_bot: bool,
    /// Whether sender is on a TLS connection.
    pub is_tls: bool,
}

impl SenderSnapshot {
    /// Build a snapshot from context with a single user read.
    ///
    /// Returns None if the user is not found (shouldn't happen for registered users).
    pub async fn build<S>(ctx: &Context<'_, S>) -> Option<Self> {
        let user_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.clone())?;
        let user = user_arc.read().await;
        Some(Self {
            nick: user.nick.clone(),
            user: user.user.clone(),
            host: user.host.clone(),
            visible_host: user.visible_host.clone(),
            realname: user.realname.clone(),
            ip: user.ip.clone(),
            account: user.account.clone(),
            is_registered: user.modes.registered,
            is_oper: user.modes.oper,
            is_bot: user.modes.bot,
            is_tls: user.modes.secure,
        })
    }

    /// Get the hostmask for shun checking (user@host).
    pub fn shun_mask(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Get the full hostmask (nick!user@visible_host).
    pub fn full_mask(&self) -> String {
        format!("{}!{}@{}", self.nick, self.user, self.visible_host)
    }

    /// Build UserContext for channel routing (extended ban checks, etc.).
    pub fn to_user_context(&self, server_name: &str) -> UserContext {
        UserContext::for_registration(crate::security::RegistrationParams {
            hostname: self.host.clone(),
            nickname: self.nick.clone(),
            username: self.user.clone(),
            realname: self.realname.clone(),
            server: server_name.to_string(),
            account: self.account.clone(),
            is_tls: self.is_tls,
            is_oper: self.is_oper,
            oper_type: None, // oper_type not yet tracked
        })
    }
}

// ============================================================================
// Shun Checking
// ============================================================================

/// Check if a user is shunned using pre-fetched snapshot.
///
/// Returns true if the user is shunned and their command should be silently ignored.
pub async fn is_shunned_with_snapshot<S>(ctx: &Context<'_, S>, snapshot: &SenderSnapshot) -> bool {
    // Check database for shuns using pre-fetched hostmask
    match ctx.db.bans().matches_shun(&snapshot.shun_mask()).await {
        Ok(Some(shun)) => {
            debug!(
                uid = %ctx.uid,
                mask = %shun.mask,
                reason = ?shun.reason,
                "Shunned user attempted to send message"
            );
            true
        }
        _ => false,
    }
}

// ============================================================================
// Message Routing Types
// ============================================================================

pub use crate::state::actor::ChannelRouteResult;

/// Options for message routing behavior.
pub struct RouteOptions {
    /// Whether to send RPL_AWAY for user targets (only PRIVMSG).
    pub send_away_reply: bool,
    /// Status prefix for channel messages (e.g. @#chan).
    pub status_prefix: Option<char>,
}

// ============================================================================
// Routing Functions
// ============================================================================

/// Check if sender can speak in a channel using pre-fetched snapshot, and broadcast if allowed.
///
/// This is the optimized version that eliminates redundant user lookups.
/// Returns the result of the routing attempt for the caller to handle errors.
pub async fn route_to_channel_with_snapshot(
    ctx: &Context<'_, crate::state::RegisteredState>,
    channel_lower: &str,
    msg: Message,
    opts: &RouteOptions,
    timestamp: Option<String>,
    msgid: Option<String>,
    snapshot: &SenderSnapshot,
) -> ChannelRouteResult {
    let channel_tx = ctx.matrix.channels.get(channel_lower).map(|c| c.clone());
    let Some(channel_tx) = channel_tx else {
        return ChannelRouteResult::NoSuchChannel;
    };

    // Build UserContext from snapshot (no user lookup needed)
    let user_context = snapshot.to_user_context(ctx.server_name());

    // Extract text and tags from message
    // TAGMSG has no text body, just tags
    let (text, tags, is_tagmsg) = match &msg.command {
        Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => {
            (text.clone(), msg.tags.clone(), false)
        }
        Command::TAGMSG(_) => (String::new(), msg.tags.clone(), true),
        _ => return ChannelRouteResult::Sent, // Should not happen
    };

    let is_notice = matches!(msg.command, Command::NOTICE(_, _));

    // Send to actor
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let event = crate::state::actor::ChannelEvent::Message {
        params: Box::new(crate::state::actor::ChannelMessageParams {
            sender_uid: ctx.uid.to_string(),
            text,
            tags,
            is_notice,
            is_tagmsg,
            user_context,
            is_registered: snapshot.is_registered,
            is_tls: ctx.state.is_tls,
            is_bot: snapshot.is_bot,
            status_prefix: opts.status_prefix,
            timestamp,
            msgid,
        }),
        reply_tx,
    };

    if (channel_tx.send(event).await).is_err() {
        return ChannelRouteResult::NoSuchChannel; // Actor died
    }

    match reply_rx.await {
        Ok(result) => result,
        Err(_) => ChannelRouteResult::NoSuchChannel,
    }
}

/// Route a message to a user target using pre-fetched snapshot, optionally sending RPL_AWAY.
///
/// This is the optimized version that eliminates redundant sender lookups.
/// Returns true if the user was found and message sent, false otherwise.
pub async fn route_to_user_with_snapshot(
    ctx: &Context<'_, crate::state::RegisteredState>,
    target_lower: &str,
    msg: Message,
    opts: &RouteOptions,
    timestamp: Option<String>,
    msgid: Option<String>,
    snapshot: &SenderSnapshot,
) -> bool {
    let target_uid = if let Some(uid) = ctx.matrix.nicks.get(target_lower) {
        uid.clone()
    } else {
        return false;
    };

    // NOTE: Spam detection is handled by validate_message_send() in validation.rs
    // before routing. No duplicate check needed here.

    // Check away status and notify sender if requested
    if opts.send_away_reply {
        let target_user_arc = ctx.matrix.users.get(&target_uid).map(|u| u.clone());
        if let Some(target_user_arc) = target_user_arc {
            let (target_nick, away_msg) = {
                let target_user = target_user_arc.read().await;
                (target_user.nick.clone(), target_user.away.clone())
            };

            if let Some(away_msg) = away_msg {
                let reply = server_reply(
                    ctx.server_name(),
                    Response::RPL_AWAY,
                    vec![
                        snapshot.nick.clone(),
                        target_nick,
                        away_msg,
                    ],
                );
                let _ = ctx.sender.send(reply).await;
            }
        }
    }

    // Check +R (registered-only PMs) - target only accepts PMs from identified users
    let target_user_arc = ctx.matrix.users.get(&target_uid).map(|u| u.clone());
    if let Some(target_user_arc) = target_user_arc {
        let target_user = target_user_arc.read().await;
        if target_user.modes.registered_only {
            // Use pre-fetched registered status from snapshot
            if !snapshot.is_registered {
                // Silently drop to avoid information leakage about +R status
                return false;
            }
        }

        // Check SILENCE list using pre-fetched sender mask from snapshot
        if !target_user.silence_list.is_empty() {
            let sender_mask = snapshot.full_mask();

            for silence_mask in &target_user.silence_list {
                if super::super::matches_hostmask(silence_mask, &sender_mask) {
                    // Silently drop the message
                    debug!(
                        target = %target_user.nick,
                        sender = %sender_mask,
                        mask = %silence_mask,
                        "Message blocked by SILENCE"
                    );
                    return false;
                }
            }
        }

        // Check +T (no CTCP) - block CTCP messages except ACTION
        if target_user.modes.no_ctcp {
            // Extract text from command to check for CTCP
            let text = match &msg.command {
                Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => Some(text.as_str()),
                _ => None,
            };
            if let Some(text) = text
                && Ctcp::is_ctcp(text)
            {
                // Check if it's an ACTION (allowed even with +T)
                if let Some(ctcp) = Ctcp::parse(text)
                    && !matches!(ctcp.kind, CtcpKind::Action)
                {
                    debug!(
                        target = %target_user.nick,
                        ctcp_type = ?ctcp.kind,
                        "CTCP blocked by +T mode"
                    );
                    return false; // Silently drop non-ACTION CTCP
                }
            }
        }
    }

    // Send to target user with appropriate tags based on their capabilities
    let timestamp = timestamp.unwrap_or_else(|| {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    });
    let msgid = msgid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Use sender's account from snapshot
    let sender_account = snapshot.account.as_ref();

    let target_sender = ctx.matrix.senders.get(&target_uid).map(|s| s.clone());
    if let Some(target_sender) = target_sender {
        // Check target's capabilities and build appropriate message
        let msg_for_target = if let Some(user_arc) = ctx.matrix.users.get(&target_uid).map(|u| u.clone()) {
            let user = user_arc.read().await;
            let has_message_tags = user.caps.contains("message-tags");
            let has_server_time = user.caps.contains("server-time");

            let mut result = msg.clone();

            // Strip label tag from recipient copies (label is sender-only)
            if ctx.label.is_some() {
                result.tags = result
                    .tags
                    .map(|tags| {
                        tags.into_iter()
                            .filter(|tag| tag.0.as_ref() != "label")
                            .collect::<Vec<_>>()
                    })
                    .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
            }

            // If recipient doesn't have message-tags, strip client-only tags
            if !has_message_tags {
                result.tags = result
                    .tags
                    .map(|tags| {
                        tags.into_iter()
                            .filter(|tag| !tag.0.starts_with('+'))
                            .collect::<Vec<_>>()
                    })
                    .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
            } else {
                // Add msgid for users with message-tags
                result = result.with_tag("msgid", Some(msgid.clone()));
            }

            // Add server-time if capability is enabled
            if has_server_time {
                result = result.with_tag("time", Some(timestamp.clone()));
            }

            // Add account-tag if sender is logged in and recipient has capability
            if let Some(account) = sender_account
                && user.caps.contains("account-tag")
            {
                result = result.with_tag("account", Some(account.clone()));
            }

            // Add bot tag if sender is a bot and recipient has message-tags
            if snapshot.is_bot && has_message_tags {
                result = result.with_tag("bot", None::<String>);
            }

            result
        } else {
            msg.clone()
        };
        let _ = target_sender.send(msg_for_target).await;
        crate::metrics::MESSAGES_SENT.inc();

        // Echo message back to sender if they have echo-message capability
        if ctx.state.capabilities.contains("echo-message") {
            let has_message_tags = ctx.state.capabilities.contains("message-tags");
            let has_server_time = ctx.state.capabilities.contains("server-time");

            let mut echo_msg = msg.clone();

            // Add msgid if sender has message-tags
            if has_message_tags {
                echo_msg = echo_msg.with_tag("msgid", Some(msgid));
            }

            // Add server-time if capability is enabled
            if has_server_time {
                echo_msg = echo_msg.with_tag("time", Some(timestamp));
            }

            // Preserve label if present
            if let Some(ref label) = ctx.label {
                echo_msg = echo_msg.with_tag("label", Some(label.clone()));
            }

            let _ = ctx.sender.send(echo_msg).await;
        }

        true
    } else {
        false
    }
}

// ============================================================================
// Error Response Helpers
// ============================================================================

/// Send ERR_CANNOTSENDTOCHAN with the given reason.
pub async fn send_cannot_send<S>(
    ctx: &Context<'_, S>,
    nick: &str,
    target: &str,
    reason: &str,
) -> HandlerResult {
    let reply = server_reply(
        ctx.server_name(),
        Response::ERR_CANNOTSENDTOCHAN,
        vec![nick.to_string(), target.to_string(), reason.to_string()],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHCHANNEL.
pub async fn send_no_such_channel<S>(ctx: &Context<'_, S>, nick: &str, target: &str) -> HandlerResult {
    let reply = Response::err_nosuchchannel(nick, target)
        .with_prefix(ctx.server_prefix());
    ctx.send_error("PRIVMSG", "ERR_NOSUCHCHANNEL", reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHNICK.
pub async fn send_no_such_nick<S>(ctx: &Context<'_, S>, nick: &str, target: &str) -> HandlerResult {
    let reply = Response::err_nosuchnick(nick, target)
        .with_prefix(ctx.server_prefix());
    ctx.send_error("PRIVMSG", "ERR_NOSUCHNICK", reply).await?;
    Ok(())
}
