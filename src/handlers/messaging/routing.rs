//! Core message routing logic.
//!
//! Handles routing messages to channels and users, including:
//! - Channel permission checks (modes, bans)
//! - User presence checks
//! - Automatic multiclient fan-out
//! - Remote user routing (S2S)

use super::delivery::build_local_recipient_message;
// use super::delivery::{send_cannot_send};
use super::multiclient::echo_to_other_sessions;
use super::types::{ChannelRouteResult, RouteMeta, RouteOptions, SenderSnapshot, UserRouteResult};
use crate::handlers::core::Context;
use crate::handlers::server_reply;
use slirc_proto::ctcp::{Ctcp, CtcpKind};
use slirc_proto::{Command, Message, Prefix, Response};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

/// Check if sender can speak in a channel using pre-fetched snapshot, and broadcast if allowed.
///
/// This is the optimized version that eliminates redundant user lookups.
/// Returns the result of the routing attempt for the caller to handle errors.
pub async fn route_to_channel_with_snapshot(
    ctx: &Context<'_, crate::state::RegisteredState>,
    channel_lower: &str,
    msg: Message,
    opts: &RouteOptions,
    meta: RouteMeta,
    snapshot: &SenderSnapshot,
) -> ChannelRouteResult {
    let RouteMeta {
        timestamp,
        msgid,
        nanotime,
        override_nick,
        relaymsg_sender_nick,
    } = meta;

    let timestamp_clone = timestamp.clone();
    let msgid_clone = msgid.clone();

    let channel_tx = ctx
        .matrix
        .channel_manager
        .channels
        .get(channel_lower)
        .map(|c| c.value().clone());
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
            nanotime: nanotime.unwrap_or_else(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            override_nick,
            relaymsg_sender_nick,
        }),
        reply_tx,
    };

    if (channel_tx.send(event).await).is_err() {
        return ChannelRouteResult::NoSuchChannel; // Actor died
    }

    let result = match reply_rx.await {
        Ok(result) => result,
        Err(_) => ChannelRouteResult::NoSuchChannel,
    };

    if result == ChannelRouteResult::Sent {
        // Self-echo to other sessions (bouncer support)
        if let (Some(ts), Some(mid)) = (&timestamp_clone, &msgid_clone) {
            echo_to_other_sessions(ctx, &msg, snapshot, ts, mid).await;
        }
    }

    result
}

/// Route a message to a user target using pre-fetched snapshot, optionally sending RPL_AWAY.
///
/// This is the optimized version that eliminates redundant sender lookups.
/// Returns the result of the routing attempt.
pub async fn route_to_user_with_snapshot(
    ctx: &Context<'_, crate::state::RegisteredState>,
    target_lower: &str,
    msg: Message,
    opts: &RouteOptions,
    timestamp: Option<String>,
    msgid: Option<String>,
    snapshot: &SenderSnapshot,
) -> UserRouteResult {
    // Deduplicate target UIDs to avoid duplicate deliveries if the nick map contains repeated entries
    let target_uids = if let Some(uids) = ctx.matrix.user_manager.nicks.get(target_lower) {
        let mut uniq: Vec<String> = Vec::new();
        for uid in uids.iter() {
            if !uniq.contains(uid) {
                uniq.push(uid.clone());
            }
        }
        uniq
    } else {
        debug!(
            "Target nick '{}' not found in nicks map. Map size: {}",
            target_lower,
            ctx.matrix.user_manager.nicks.len()
        );
        return UserRouteResult::NoSuchNick;
    };

    // NOTE: Spam detection is handled by validate_message_send() in validation.rs
    // before routing. No duplicate check needed here.

    // For bouncer support: send to ALL UIDs with this nick
    let mut sent_count = 0;
    let mut delivered_local: HashSet<String> = HashSet::new();
    let mut blocked_by_regged_only = false;
    let mut blocked_by_silence = false;

    // Precompute msgid/time once for this fan-out
    let timestamp_str = timestamp.clone().unwrap_or_else(|| {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    });
    let msgid_str = msgid
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    for target_uid in &target_uids {
        // Check away status and notify sender if requested (only once, not per UID)
        if opts.send_away_reply && sent_count == 0 {
            let target_user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(target_uid)
                .map(|u| u.value().clone());
            if let Some(target_user_arc) = target_user_arc {
                let (target_nick, away_msg) = {
                    let target_user = target_user_arc.read().await;
                    (target_user.nick.clone(), target_user.away.clone())
                };

                if let Some(away_msg) = away_msg {
                    let reply = server_reply(
                        ctx.server_name(),
                        Response::RPL_AWAY,
                        vec![snapshot.nick.clone(), target_nick, away_msg],
                    );
                    let _ = ctx.sender.send(reply).await;
                }
            }
        }

        // Check +R (registered-only PMs) - target only accepts PMs from identified users
        let target_user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(target_uid)
            .map(|u| u.value().clone());
        if let Some(target_user_arc) = target_user_arc {
            let target_user = target_user_arc.read().await;
            debug!(
                "Checking +R for target {}: registered_only={}, sender_registered={}",
                target_user.nick, target_user.modes.registered_only, snapshot.is_registered
            );
            if target_user.modes.registered_only {
                // Use pre-fetched registered status from snapshot
                if !snapshot.is_registered {
                    // Check ACCEPT list (Caller ID) override
                    // If sender is in accept list, allow the message even if not registered
                    let sender_nick_lower = slirc_proto::irc_to_lower(&snapshot.nick);
                    if !target_user.accept_list.contains(&sender_nick_lower) {
                        debug!("Blocked by +R for UID {}", target_uid);
                        blocked_by_regged_only = true;
                        continue; // Skip this UID
                    }
                }
            }

            // Check SILENCE list using pre-fetched sender mask from snapshot
            if !target_user.silence_list.is_empty() {
                let sender_mask = snapshot.full_mask();

                let mut is_locally_silenced = false;
                for silence_mask in &target_user.silence_list {
                    if crate::handlers::matches_hostmask(silence_mask, &sender_mask) {
                        // Silently drop the message
                        debug!(
                            target = %target_user.nick,
                            sender = %sender_mask,
                            mask = %silence_mask,
                            "Message blocked by SILENCE"
                        );
                        is_locally_silenced = true;
                        break;
                    }
                }
                if is_locally_silenced {
                    blocked_by_silence = true;
                    continue; // Skip this UID
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
                        continue; // Skip this UID
                    }
                }
            }
        }

        // Use sender's account from snapshot
        let sender_account = snapshot.account.as_ref();

        // Check if target is a service user
        let is_service = if let Some(user_arc) = ctx.matrix.user_manager.users.get(target_uid) {
            user_arc.read().await.modes.service
        } else {
            false
        };

        if is_service {
            // Route to service manager
            let text = match &msg.command {
                Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => text.as_str(),
                _ => continue, // Ignore other commands for services
            };

            let handled = crate::services::route_service_message(
                ctx.matrix,
                ctx.uid,
                &snapshot.nick,
                target_lower,
                text,
                &ctx.sender,
            )
            .await;

            if handled {
                sent_count += 1;
                continue;
            }
        }

        // Check if target is local or remote
        if ctx
            .matrix
            .user_manager
            .get_first_sender(target_uid)
            .is_some()
        {
            // LOCAL USER: Check target's capabilities and build appropriate message
            // LOCAL USER: deliver to all sessions with per-session caps
            if let Some(sessions) = ctx.matrix.user_manager.get_senders_cloned(target_uid) {
                let mut any_sent = false;
                for sess in sessions {
                    let caps = ctx
                        .matrix
                        .user_manager
                        .get_session_caps(sess.session_id)
                        .unwrap_or_default();
                    let msg_for_target = build_local_recipient_message(
                        &msg,
                        &caps,
                        snapshot,
                        &msgid_str,
                        &timestamp_str,
                        ctx.label.as_ref(),
                    );
                    let _ = sess.tx.send(Arc::new(msg_for_target)).await;
                    any_sent = true;
                    crate::metrics::MESSAGES_SENT.inc();
                    sent_count += 1;
                }
                if any_sent {
                    delivered_local.insert(target_uid.clone());
                }
            }
        } else {
            // REMOTE USER: Route via SyncManager
            // Construct S2S message: :SourceUID PRIVMSG TargetUID :text
            let text = match &msg.command {
                Command::PRIVMSG(_, text) => text,
                Command::NOTICE(_, text) => text,
                _ => continue,
            };

            let cmd = match &msg.command {
                Command::PRIVMSG(_, _) => Command::PRIVMSG(target_uid.clone(), text.clone()),
                Command::NOTICE(_, _) => Command::NOTICE(target_uid.clone(), text.clone()),
                _ => continue,
            };

            let mut routed_msg = Message {
                tags: msg.tags.clone(),                      // Preserve tags
                prefix: Some(Prefix::new_from_str(ctx.uid)), // Use UID as source
                command: cmd,
            };

            // Add metadata tags
            routed_msg = routed_msg.with_tag("msgid", Some(msgid_str.clone()));
            routed_msg = routed_msg.with_tag("time", Some(timestamp_str.clone()));
            if let Some(account) = sender_account {
                routed_msg = routed_msg.with_tag("account", Some(account.clone()));
            }

            if ctx
                .matrix
                .sync_manager
                .route_to_remote_user(target_uid, Arc::new(routed_msg))
                .await
            {
                sent_count += 1;
            }
        }
    }

    // Account cluster self-echo: forward the sent message to other local sessions on the same account
    if sent_count > 0
        && ctx.matrix.config.multiclient.enabled
        && let Some(account) = &snapshot.account
    {
        let account_lower = slirc_proto::irc_to_lower(account);

        // Collect candidate local users to avoid holding locks while awaiting
        let sibling_candidates: Vec<_> = ctx
            .matrix
            .user_manager
            .users
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (sibling_uid, sibling_arc) in sibling_candidates {
            if sibling_uid == ctx.uid {
                continue;
            }
            if delivered_local.contains(&sibling_uid) {
                continue;
            }

            let sibling = sibling_arc.read().await;
            if sibling
                .account
                .as_ref()
                .map(|a| slirc_proto::irc_to_lower(a))
                != Some(account_lower.clone())
            {
                continue;
            }

            if let Some(sessions) = ctx.matrix.user_manager.get_senders_cloned(&sibling_uid) {
                let mut delivered_any = false;
                for sess in sessions {
                    let caps = ctx
                        .matrix
                        .user_manager
                        .get_session_caps(sess.session_id)
                        .unwrap_or_default();
                    let msg_for_sibling = build_local_recipient_message(
                        &msg,
                        &caps,
                        snapshot,
                        &msgid_str,
                        &timestamp_str,
                        None, // self-echo copies never carry labels
                    );
                    let _ = sess.tx.send(Arc::new(msg_for_sibling)).await;
                    delivered_any = true;
                    crate::metrics::MESSAGES_SENT.inc();
                    sent_count += 1;
                }
                if delivered_any {
                    delivered_local.insert(sibling_uid.clone());
                }
            }
        }
    }

    // After loop: Send echo message ONCE if sender has echo-message capability and we sent to at least one UID
    if sent_count > 0 && ctx.state.capabilities.contains("echo-message") {
        let has_message_tags = ctx.state.capabilities.contains("message-tags");
        let has_server_time = ctx.state.capabilities.contains("server-time");

        let mut echo_msg = msg.clone();

        // Add msgid if sender has message-tags
        if has_message_tags {
            echo_msg = echo_msg.with_tag("msgid", Some(msgid_str.clone()));
        }

        // Add server-time if capability is enabled
        if has_server_time {
            echo_msg = echo_msg.with_tag("time", Some(timestamp_str.clone()));
        }

        // Preserve label if present
        if let Some(ref label) = ctx.label {
            echo_msg = echo_msg.with_tag("label", Some(label.clone()));
        }

        let _ = ctx.sender.send(echo_msg).await;
    }

    if sent_count > 0 {
        // Self-echo to other sessions (bouncer support)
        if let (Some(ts), Some(mid)) = (&timestamp, &msgid) {
            echo_to_other_sessions(ctx, &msg, snapshot, ts, mid).await;
        }
        UserRouteResult::Sent
    } else if blocked_by_regged_only {
        UserRouteResult::BlockedRegisteredOnly
    } else if blocked_by_silence {
        UserRouteResult::BlockedSilence
    } else {
        UserRouteResult::NoSuchNick
    }
}
