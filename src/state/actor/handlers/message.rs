//! PRIVMSG/NOTICE routing through channels.
//!
//! Validates message delivery against bans, moderation, and member status.

use super::super::validation::{create_user_mask, is_banned};
use super::{ChannelActor, ChannelMessageParams, ChannelMode, ChannelRouteResult};
use slirc_proto::message::Tag;
use slirc_proto::{Command, Message};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::trace;

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

        // Check +C (no CTCP)
        if modes.contains(&ChannelMode::NoCtcp)
            && slirc_proto::ctcp::Ctcp::is_ctcp(&text)
            && let Some(ctcp) = slirc_proto::ctcp::Ctcp::parse(&text)
            && !matches!(ctcp.kind, slirc_proto::ctcp::CtcpKind::Action)
        {
            let _ = reply_tx.send(ChannelRouteResult::BlockedCTCP);
            return;
        }

        // Check bans (+b) and quiets (+q)
        let is_op = self.member_has_halfop_or_higher(&sender_uid);
        let user_mask = create_user_mask(&user_context);

        if !is_op {
            if is_banned(&user_mask, &user_context, &self.bans, &self.excepts) {
                let _ = reply_tx.send(ChannelRouteResult::BlockedBanned);
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

        // Broadcast
        // Strip colors/formatting if +c mode is set
        let text = if modes.contains(&ChannelMode::NoColors) && !is_tagmsg {
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
                (false, true) => Command::NOTICE(target, text),
                (false, false) => Command::PRIVMSG(target, text),
            },
        };

        let mut recipients_sent = 0usize;
        let mut extra_delivered: HashSet<String> = HashSet::new();

        // Check if sender has echo-message capability for self-echo
        let sender_caps = self.user_caps.get(&sender_uid);
        let sender_has_echo = has_caps(sender_caps, "echo-message");

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

            if let Some(ref account) = user_context.account {
                if has_caps(target_caps, "account-tag") {
                    recipient_tags.push(Tag(Cow::Borrowed("account"), Some(account.clone())));
                }
            }

            if let Some(ref relay_nick) = relaymsg_sender_nick {
                if has_caps(target_caps, "draft/relaymsg") {
                    recipient_tags.push(Tag(
                        Cow::Owned("draft/relaymsg".to_string()),
                        Some(relay_nick.clone()),
                    ));
                }
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

        for (uid, sender) in &self.senders {
            let user_caps = self.user_caps.get(uid);
            let mut should_fanout = uid == &sender_uid;

            // If +U is set, and sender is NOT privileged, only send to privileged members
            if is_op_moderated && !sender_is_privileged {
                let recipient_privileged = self.member_has_voice_or_higher(uid);
                if !recipient_privileged && uid != &sender_uid {
                    continue;
                }
            }

            if uid == &sender_uid {
                // Echo back to sender if they have echo-message capability
                if !sender_has_echo && override_nick.is_none() {
                    // No echo; still fan out to other sessions on this account
                } else {
                    let has_message_tags = has_caps(user_caps, "message-tags");
                    let has_server_time = has_caps(user_caps, "server-time");

                    let mut echo_msg = base_msg.clone();
                    echo_msg.tags =
                        build_echo_tags(&tags, &timestamp, &msgid, has_message_tags, has_server_time);

                    if let Err(err) = sender.try_send(Arc::new(echo_msg)) {
                        trace!("Failed to send echo to {}: {:?}", uid, err);
                        match err {
                            TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                            TrySendError::Closed(_) => {}
                        }
                    }
                    recipients_sent += 1;
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
                                modes.voice || modes.halfop || modes.op || modes.admin || modes.owner
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

                // Check for TAGMSG filtering requirement
                let has_message_tags = has_caps(user_caps, "message-tags");
                if is_tagmsg && !has_message_tags {
                    continue;
                }

                let recipient_msg = build_msg_for_recipient(uid, user_caps);

                if let Err(err) = sender.try_send(Arc::new(recipient_msg)) {
                    trace!("Failed to send message to {}: {:?}", uid, err);
                    match err {
                        TrySendError::Full(_) => self.request_disconnect(uid, "SendQ exceeded"),
                        TrySendError::Closed(_) => {}
                    }
                }
                recipients_sent += 1;
                should_fanout = true;
            }
            // Innovation: Account Fan-out (Multiclient)
            // If the recipient (or sender) is a local user, fan out to other local sessions
            if should_fanout {
                let is_remote = !uid.starts_with(self.server_id.as_str());

                if multiclient_enabled && !is_remote && !extra_delivered.contains(uid) {
                    if let Some(matrix) = &matrix {
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

                                    let sibling_caps = sibling.caps.clone();
                                    drop(sibling);

                                    let sib_has_msg_tags =
                                        has_caps(Some(&sibling_caps), "message-tags");
                                    if is_tagmsg && !sib_has_msg_tags {
                                        continue;
                                    }

                                    if let Some(sibling_sender) = matrix
                                        .user_manager
                                        .senders
                                        .get(&sibling_uid)
                                        .map(|s| s.value().clone())
                                    {
                                        let sibling_msg =
                                            build_msg_for_recipient(&sibling_uid, Some(&sibling_caps));
                                        let _ = sibling_sender.send(Arc::new(sibling_msg)).await;
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

        let _ = reply_tx.send(ChannelRouteResult::Sent);
    }
}
