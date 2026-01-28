//! PRIVMSG/NOTICE routing through channels.
//!
//! Validates message delivery against bans, moderation, and member status.

use super::super::validation::{create_user_mask, is_banned};
use super::{ChannelActor, ChannelMessageParams, ChannelMode, ChannelRouteResult};
use governor::{Quota, RateLimiter as GovRateLimiter};
use slirc_proto::message::Tag;
use slirc_proto::{Command, Message};
use std::borrow::Cow;
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// Build tags for echo-message response based on sender capabilities.
fn build_echo_tags(
    tags: &Option<Vec<Tag>>,
    timestamp: &str,
    msgid: &str,
    has_message_tags: bool,
    has_server_time: bool,
) -> Option<Vec<Tag>> {
    let mut echo_tags: Vec<Tag> = Vec::with_capacity(4); // time, msgid, label, maybe 1 client tag

    // Add server-time if sender has the capability
    if has_server_time {
        echo_tags.push(Tag(
            Cow::Owned("time".to_string()),
            Some(timestamp.to_string()),
        ));
    }

    // If sender has message-tags, include client-only tags and msgid
    if has_message_tags {
        // Preserve client-only tags from original message
        if let Some(orig_tags) = tags {
            for tag in orig_tags {
                if tag.0.starts_with('+') {
                    echo_tags.push(tag.clone());
                }
            }
        }
        // Add msgid
        echo_tags.push(Tag(
            Cow::Owned("msgid".to_string()),
            Some(msgid.to_string()),
        ));
    }

    // Always preserve the label tag if present (for labeled-response)
    if let Some(orig_tags) = tags {
        for tag in orig_tags {
            if tag.0.as_ref() == "label" {
                echo_tags.push(tag.clone());
                break;
            }
        }
    }

    if echo_tags.is_empty() {
        None
    } else {
        Some(echo_tags)
    }
}

/// Check if user has required caps.
fn has_caps(caps: Option<&HashSet<String>>, required: &str) -> bool {
    caps.map(|c| c.contains(required)).unwrap_or(false)
}

impl ChannelActor {
    #[allow(clippy::collapsible_if)]
    pub(crate) async fn handle_message(
        &mut self,
        params: ChannelMessageParams,
        reply_tx: oneshot::Sender<ChannelRouteResult>,
    ) {
        let ChannelMessageParams {
            sender_uid,
            text,
            tags,
            is_notice,
            is_tagmsg,
            user_context,
            is_registered,
            is_tls,
            is_bot,
            status_prefix,
            timestamp,
            msgid,
            override_nick,
            relaymsg_sender_nick,
            nanotime,
        } = params;

        let is_member = self.members.contains_key(&sender_uid);
        let modes = &self.modes;

        // Check +n (no external messages)
        if modes.contains(&ChannelMode::NoExternal) && !is_member {
            let _ = reply_tx.send(ChannelRouteResult::BlockedExternal);
            return;
        }

        // Check +r (registered-only channel)
        if (modes.contains(&ChannelMode::Registered)
            || modes.contains(&ChannelMode::RegisteredOnly))
            && !is_registered
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedRegisteredOnly);
            return;
        }

        // Check +z (TLS-only channel)
        if modes.contains(&ChannelMode::TlsOnly) && !is_tls {
            let _ = reply_tx.send(ChannelRouteResult::BlockedExternal);
            return;
        }

        // Check +m (moderated)
        if modes.contains(&ChannelMode::Moderated) && !self.member_has_voice_or_higher(&sender_uid)
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedModerated);
            return;
        }

        // Check +M (Moderated-Unregistered)
        if modes.contains(&ChannelMode::ModeratedUnreg)
            && !is_registered
            && !self.member_has_voice_or_higher(&sender_uid)
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedRegisteredSpeak);
            return;
        }

        // Check +T (no notice)
        if is_notice
            && modes.contains(&ChannelMode::NoNotice)
            && !self.member_has_halfop_or_higher(&sender_uid)
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedNotice);
            return;
        }

        if modes.contains(&ChannelMode::NoCtcp)
            && slirc_proto::ctcp::Ctcp::is_ctcp(&text)
            && let Some(ctcp) = slirc_proto::ctcp::Ctcp::parse(&text)
            && !matches!(ctcp.kind, slirc_proto::ctcp::CtcpKind::Action)
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedCTCP);
            return;
        }

        // Check +B (Anti-caps)
        if modes.contains(&ChannelMode::AntiCaps) && text.len() > 10 && !is_tagmsg {
            let caps = text.chars().filter(|c| c.is_uppercase()).count();
            let total = text.chars().filter(|c| c.is_alphabetic()).count();
            if total > 0 && (caps as f32 / total as f32) > 0.7 {
                let _ = reply_tx.send(ChannelRouteResult::BlockedAntiCaps);
                return;
            }
        }

        // Check +G (Censor)
        if modes.contains(&ChannelMode::Censor) && !is_tagmsg {
            let matrix = self.matrix.upgrade();
            let censored_words = matrix
                .as_ref()
                .map(|m| &m.config.security.spam.censored_words);

            if let Some(words) = censored_words {
                let text_lower = text.to_lowercase();
                for word in words {
                    if text_lower.contains(&word.to_lowercase()) {
                        let _ = reply_tx.send(ChannelRouteResult::BlockedCensored);
                        return;
                    }
                }
            }
        }

        // Check bans (+b) and quiets (+q)
        let is_op = self.member_has_halfop_or_higher(&sender_uid);
        let user_mask = create_user_mask(&user_context);

        if !is_op {
            if is_banned(&user_mask, &user_context, &self.bans, &self.excepts) {
                let _ = reply_tx.send(ChannelRouteResult::BlockedBanned);
                return;
            }

            // Check +f (Flood protection)
            let is_flooding = if let Some(param) = self.flood_config.get(&super::FloodType::Message)
            {
                let limiter = self
                    .flood_message_limiters
                    .entry(sender_uid.clone())
                    .or_insert_with(|| {
                        let mut period_per_token = if param.count == 0 {
                            Duration::from_secs(param.period as u64) // Fallback if count is 0 (should not happen)
                        } else {
                            Duration::from_secs_f64(param.period as f64 / param.count as f64)
                        };

                        // Fix: Clamp to 1 nanosecond to prevent panic in Quota::with_period
                        // if period is small and count is very large (DoS protection).
                        if period_per_token.is_zero() {
                            period_per_token = Duration::from_nanos(1);
                        }

                        let quota = Quota::with_period(period_per_token)
                            .unwrap_or_else(|| Quota::with_period(Duration::from_nanos(1)).unwrap())
                            .allow_burst(NonZeroU32::new(param.count).unwrap_or(NonZeroU32::MIN));
                        GovRateLimiter::direct(quota)
                    });

                limiter.check().is_err()
            } else {
                false
            };

            if is_flooding {
                // Flood detected! KICK the user.
                let kick_msg = Message {
                    tags: None,
                    prefix: Some(slirc_proto::Prefix::new(
                        self.server_id.to_string(),
                        "system".to_string(),
                        self.server_id.to_string(),
                    )),
                    command: Command::KICK(
                        self.name.clone(),
                        user_context.nickname.clone(),
                        Some("Channel flood triggered (+f)".to_string()),
                    ),
                };

                // Remove user state
                self.members.remove(&sender_uid);
                self.senders.remove(&sender_uid);
                self.user_nicks.remove(&sender_uid);
                self.user_caps.remove(&sender_uid);
                self.flood_message_limiters.remove(&sender_uid);

                self.handle_broadcast(kick_msg, None).await;
                self.cleanup_if_empty();

                let _ = reply_tx.send(ChannelRouteResult::NoSuchChannel);
                return;
            }

            // Check m: extbans (mute)
            // Voiced users are immune to m: bans
            if !self.member_has_voice_or_higher(&sender_uid) {
                for ban in &self.bans {
                    #[allow(clippy::collapsible_if)]
                    if let Some(mask) = ban.mask.strip_prefix("m:") {
                        if crate::security::matches_ban_or_except(mask, &user_mask, &user_context) {
                            let is_excepted = self.excepts.iter().any(|e| {
                                if crate::security::matches_ban_or_except(
                                    &e.mask,
                                    &user_mask,
                                    &user_context,
                                ) {
                                    return true;
                                }
                                if let Some(e_mask) = e.mask.strip_prefix("m:") {
                                    return crate::security::matches_ban_or_except(
                                        e_mask,
                                        &user_mask,
                                        &user_context,
                                    );
                                }
                                false
                            });
                            if !is_excepted {
                                let _ = reply_tx.send(ChannelRouteResult::BlockedBanned);
                                return;
                            }
                        }
                    }
                }
            }

            for quiet in &self.quiets {
                if crate::security::matches_ban_or_except(&quiet.mask, &user_mask, &user_context) {
                    let is_excepted = self.excepts.iter().any(|e| {
                        crate::security::matches_ban_or_except(&e.mask, &user_mask, &user_context)
                    });
                    if !is_excepted {
                        let _ = reply_tx.send(ChannelRouteResult::BlockedModerated);
                        return;
                    }
                }
            }
        }

        // Strip colors/formatting if +c or +S mode is set
        let text = if (modes.contains(&ChannelMode::NoColors)
            || modes.contains(&ChannelMode::StripColors))
            && !is_tagmsg
        {
            use slirc_proto::colors::FormattedStringExt;
            text.strip_formatting().into_owned()
        } else {
            text
        };

        // Generate server-side tags
        let timestamp = timestamp.unwrap_or_else(|| {
            chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        });
        let msgid = msgid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Build target with status prefix if present (for STATUSMSG)
        let target = if let Some(prefix) = status_prefix {
            format!("{}{}", prefix, self.name)
        } else {
            self.name.clone()
        };

        let base_msg = Message {
            tags: tags.clone(),
            prefix: Some(slirc_proto::Prefix::new(
                override_nick
                    .as_ref()
                    .unwrap_or(&user_context.nickname)
                    .clone(),
                user_context.username.clone(),
                user_context.hostname.clone(),
            )),
            command: match (is_tagmsg, is_notice) {
                (true, _) => Command::TAGMSG(target),
                (false, true) => Command::NOTICE(target, text.clone()),
                (false, false) => Command::PRIVMSG(target, text.clone()),
            },
        };

        let mut recipients_sent = 0usize;
        let mut extra_delivered: HashSet<String> = HashSet::new();

        // Sender echo is computed per session; we'll handle per-session caps below

        let matrix = self.matrix.upgrade();
        let multiclient_enabled = matrix
            .as_ref()
            .map(|m| m.config.multiclient.enabled)
            .unwrap_or(false);

        // Check +U (Op Moderated)
        let is_op_moderated = modes.contains(&ChannelMode::OpModerated);
        let sender_is_privileged = self.member_has_voice_or_higher(&sender_uid);

        // Helper to build message for a specific recipient based on their capabilities
        let build_msg_for_recipient = |target_uid: &str,
                                       target_caps: Option<&HashSet<String>>|
         -> Message {
            let mut msg = base_msg.clone();

            let has_message_tags = has_caps(target_caps, "message-tags");
            let has_server_time = has_caps(target_caps, "server-time");

            // For TAGMSG, only send to recipients with message-tags capability
            if is_tagmsg && !has_message_tags {
                // We return original msg, but caller checks tagmsg logic?
                // Actually this check was outside construction.
                // We'll handle tagmsg check in caller.
                return msg;
            }

            // Build recipient's tags based on their capabilities
            let mut recipient_tags: Vec<Tag> = Vec::with_capacity(5);

            if has_server_time {
                recipient_tags.push(Tag(Cow::Borrowed("time"), Some(timestamp.clone())));
            }

            if has_message_tags {
                if let Some(ref orig_tags) = tags {
                    for tag in orig_tags {
                        if tag.0.starts_with('+') {
                            recipient_tags.push(tag.clone());
                        }
                    }
                }
                recipient_tags.push(Tag(Cow::Borrowed("msgid"), Some(msgid.clone())));
            }

            if let Some(ref account) = user_context.account
                && has_caps(target_caps, "account-tag")
            {
                recipient_tags.push(Tag(Cow::Borrowed("account"), Some(account.clone())));
            }

            if let Some(ref relay_nick) = relaymsg_sender_nick
                && has_caps(target_caps, "draft/relaymsg")
            {
                recipient_tags.push(Tag(
                    Cow::Owned("draft/relaymsg".to_string()),
                    Some(relay_nick.clone()),
                ));
            }

            if is_bot && has_message_tags {
                recipient_tags.push(Tag(Cow::Borrowed("bot"), None));
            }

            // Innovation 2: Routing tags for remote users
            let is_target_remote = !target_uid.starts_with(self.server_id.as_str());
            if is_target_remote {
                recipient_tags.push(Tag(
                    Cow::Owned("x-target-uid".to_string()),
                    Some(target_uid.to_string()),
                ));
                if let Command::PRIVMSG(target, _) | Command::NOTICE(target, _) = &base_msg.command
                {
                    recipient_tags.push(Tag(
                        Cow::Owned("x-visible-target".to_string()),
                        Some(target.clone()),
                    ));
                }
            }

            msg.tags = if recipient_tags.is_empty() {
                None
            } else {
                Some(recipient_tags)
            };
            msg
        };

        // Collect member UIDs - we'll use matrix.user_manager.try_send_to_uid() for multi-session support
        let member_uids: Vec<String> = self.members.keys().cloned().collect();

        for uid in &member_uids {
            let _user_caps = self.user_caps.get(uid);
            let mut should_fanout = uid == &sender_uid;

            // If +U is set, and sender is NOT privileged, only send to privileged members
            if is_op_moderated && !sender_is_privileged {
                let recipient_privileged = self.member_has_voice_or_higher(uid);
                if !recipient_privileged && uid != &sender_uid {
                    continue;
                }
            }

            if uid == &sender_uid {
                // Echo to each sender session based on per-session capabilities
                if let Some(ref matrix) = matrix {
                    if let Some(sessions) = matrix.user_manager.get_senders_cloned(uid) {
                        let mut any_sent = false;
                        for sess in sessions {
                            let caps = matrix
                                .user_manager
                                .get_session_caps(sess.session_id)
                                .unwrap_or_default();
                            let has_echo = caps.contains("echo-message");
                            if !has_echo && override_nick.is_none() {
                                continue;
                            }
                            let has_message_tags = caps.contains("message-tags");
                            let has_server_time = caps.contains("server-time");
                            let mut echo_msg = base_msg.clone();
                            echo_msg.tags = build_echo_tags(
                                &tags,
                                &timestamp,
                                &msgid,
                                has_message_tags,
                                has_server_time,
                            );
                            let _ = sess.tx.try_send(Arc::new(echo_msg));
                            any_sent = true;
                            recipients_sent += 1;
                        }
                        if !any_sent {
                            // No echo; still fan out to other sessions on this account
                        }
                    }
                }
            } else {
                if let Some(prefix) = status_prefix {
                    if let Some(modes) = self.members.get(uid) {
                        // Check if recipient has the required status level for STATUSMSG
                        // Each prefix sends to users with that status or higher
                        let has_status = match prefix {
                            '~' => modes.owner,
                            '&' => modes.admin || modes.owner,
                            '@' => modes.op || modes.admin || modes.owner,
                            '%' => modes.halfop || modes.op || modes.admin || modes.owner,
                            '+' => {
                                modes.voice
                                    || modes.halfop
                                    || modes.op
                                    || modes.admin
                                    || modes.owner
                            }
                            _ => false,
                        };
                        if !has_status {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                // Fan out to each recipient session using per-session caps
                if let Some(ref matrix) = matrix {
                    if let Some(sessions) = matrix.user_manager.get_senders_cloned(uid) {
                        let mut any_sent = false;
                        for sess in sessions {
                            let caps = matrix
                                .user_manager
                                .get_session_caps(sess.session_id)
                                .unwrap_or_default();
                            let has_message_tags = caps.contains("message-tags");
                            if is_tagmsg && !has_message_tags {
                                continue;
                            }
                            let recipient_msg = build_msg_for_recipient(uid, Some(&caps));
                            let _ = sess.tx.try_send(Arc::new(recipient_msg));
                            recipients_sent += 1;
                            any_sent = true;
                        }
                        if any_sent {
                            should_fanout = true;
                        }
                    }
                }
            }
            // Innovation: Account Fan-out (Multiclient)
            // If the recipient (or sender) is a local user, fan out to other local sessions
            if should_fanout {
                let is_remote = !uid.starts_with(self.server_id.as_str());

                if multiclient_enabled
                    && !is_remote
                    && !extra_delivered.contains(uid)
                    && let Some(matrix) = &matrix
                {
                    // Resolve account for this UID without holding DashMap locks across await
                    let account_opt = matrix
                        .user_manager
                        .users
                        .get(uid)
                        .map(|u| u.value().clone());

                    if let Some(user_arc) = account_opt {
                        let account = user_arc.read().await.account.clone();
                        if let Some(account) = account {
                            let account_lower = slirc_proto::irc_to_lower(&account);

                            let sibling_candidates: Vec<_> = matrix
                                .user_manager
                                .users
                                .iter()
                                .map(|e| (e.key().clone(), e.value().clone()))
                                .collect();

                            for (sibling_uid, sibling_arc) in sibling_candidates {
                                if &sibling_uid == uid {
                                    continue;
                                }

                                if !sibling_uid.starts_with(self.server_id.as_str()) {
                                    continue;
                                }

                                if self.senders.contains_key(&sibling_uid) {
                                    continue;
                                }

                                if extra_delivered.contains(&sibling_uid) {
                                    continue;
                                }

                                let sibling = sibling_arc.read().await;
                                let sibling_account = sibling
                                    .account
                                    .as_ref()
                                    .map(|account| slirc_proto::irc_to_lower(account));
                                if sibling_account != Some(account_lower.clone()) {
                                    continue;
                                }

                                // Deliver to all sessions for the sibling UID based on per-session caps
                                drop(sibling);
                                if let Some(sessions) =
                                    matrix.user_manager.get_senders_cloned(&sibling_uid)
                                {
                                    let mut delivered_any = false;
                                    for sess in sessions {
                                        let caps = matrix
                                            .user_manager
                                            .get_session_caps(sess.session_id)
                                            .unwrap_or_default();
                                        let sib_has_msg_tags = caps.contains("message-tags");
                                        if is_tagmsg && !sib_has_msg_tags {
                                            continue;
                                        }
                                        let sibling_msg =
                                            build_msg_for_recipient(&sibling_uid, Some(&caps));
                                        let _ = sess.tx.try_send(Arc::new(sibling_msg));
                                        delivered_any = true;
                                        recipients_sent += 1;
                                    }
                                    if delivered_any {
                                        extra_delivered.insert(sibling_uid.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Record message fan-out metric (Innovation 3)
        if recipients_sent > 0 {
            crate::metrics::record_fanout(recipients_sent);
        }

        // S2S Routing: Fan out to remote servers
        // We optimize by resolving the next-hop for each remote member and deduplicating
        // so we only send one copy per outbound link (Hop Collapsing).
        if let Some(matrix) = self.matrix.upgrade() {
            let sender_sid = if sender_uid.len() >= 3 {
                &sender_uid[0..3]
            } else {
                // Should not happen for valid UIDs
                self.server_id.as_str()
            };

            let mut target_peers = std::collections::HashSet::new();
            let my_sid = self.server_id.as_str();

            for uid in &member_uids {
                // Skip local users
                if uid.starts_with(my_sid) {
                    continue;
                }

                // Split Horizon: Do not echo back to the sender's origin server
                if uid.starts_with(sender_sid) {
                    continue;
                }

                // Resolve next hop for this remote member
                let target_sid_str = &uid[0..3];
                let target_sid = slirc_proto::sync::ServerId::new(target_sid_str.to_string());

                // Walk topology to find the direct peer (next hop)
                let mut current = target_sid.clone();
                for _ in 0..20 {
                    // Max depth protection
                    if matrix.sync_manager.links.contains_key(&current) {
                        target_peers.insert(current);
                        break;
                    }
                    if let Some(parent) = matrix.sync_manager.topology.get_route(&current) {
                        current = parent;
                    } else {
                        break;
                    }
                }
            }

            if !target_peers.is_empty() {
                // Construct Canonical S2S Message
                let mut s2s_msg = base_msg.clone();
                s2s_msg.prefix = Some(slirc_proto::Prefix::new_from_str(&sender_uid)); // Use UID as source

                // Standard S2S Tags
                let mut s2s_tags = vec![
                    Tag(Cow::Borrowed("time"), Some(timestamp.clone())),
                    Tag(Cow::Borrowed("msgid"), Some(msgid.clone())),
                ];
                if let Some(ref account) = user_context.account {
                    s2s_tags.push(Tag(Cow::Borrowed("account"), Some(account.clone())));
                }
                // Preserve original tags
                if let Some(ref orig_tags) = tags {
                    for tag in orig_tags {
                        if !tag.0.starts_with("x-") {
                            // Filter internal tags if any
                            s2s_tags.push(tag.clone());
                        }
                    }
                }
                s2s_msg.tags = Some(s2s_tags);

                let s2s_msg = Arc::new(s2s_msg);

                for peer_sid in target_peers {
                    if let Some(link) = matrix.sync_manager.get_peer_for_server(&peer_sid) {
                        let _ = link.tx.try_send(s2s_msg.clone());
                    }
                }
            }
        }
        if self.silent_members.remove(&sender_uid) {
            let join_msg = Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::new(
                    user_context.nickname.clone(),
                    user_context.username.clone(),
                    user_context.hostname.clone(),
                )),
                command: Command::JOIN(self.name.clone(), None, None),
            };
            self.handle_broadcast(join_msg, Some(sender_uid.clone()))
                .await;
        }

        // Store message in history (Issue 5)
        if let Some(matrix) = self.matrix.upgrade() {
            let history = matrix.service_manager.history.clone();
            let target_name = self.name.clone();
            let now = nanotime;

            let command = match (is_tagmsg, is_notice) {
                (true, _) => "TAGMSG".to_string(),
                (false, true) => "NOTICE".to_string(),
                (false, false) => "PRIVMSG".to_string(),
            };

            let prefix = format!(
                "{}!{}@{}",
                user_context.nickname, user_context.username, user_context.hostname
            );

            let history_tags = tags.clone().map(|t| {
                t.into_iter()
                    .map(|msg_tag| crate::history::types::MessageTag {
                        key: msg_tag.0.to_string(),
                        value: msg_tag.1.map(|v| v.to_string()),
                    })
                    .collect()
            });

            let envelope = crate::history::types::MessageEnvelope {
                command,
                prefix,
                target: target_name.clone(),
                text: text.clone(),
                tags: history_tags,
            };

            let stored_msg = crate::history::types::StoredMessage {
                msgid: msgid.clone(),
                target: slirc_proto::irc_to_lower(&target_name),
                sender: user_context.nickname.clone(),
                envelope,
                nanotime: now,
                account: user_context.account.clone(),
                status_prefix: params.status_prefix,
            };

            let item = crate::history::types::HistoryItem::Message(stored_msg);

            tokio::spawn(async move {
                if let Err(e) = history.store_item(&target_name, item).await {
                    tracing::error!("Failed to store history channel={}: {}", target_name, e);
                }
            });
        }

        let _ = reply_tx.send(ChannelRouteResult::Sent);
    }
}
