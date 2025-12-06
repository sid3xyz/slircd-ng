use super::super::validation::{create_user_mask, is_banned};
use super::{ChannelActor, ChannelMode, ChannelRouteResult, Uid};
use crate::security::UserContext;
use slirc_proto::message::Tag;
use slirc_proto::{Command, Message};
use std::sync::Arc;
use tokio::sync::oneshot;

impl ChannelActor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_message(
        &mut self,
        sender_uid: Uid,
        text: String,
        tags: Option<Vec<Tag>>,
        is_notice: bool,
        user_context: UserContext,
        is_registered: bool,
        is_tls: bool,
        status_prefix: Option<char>,
        reply_tx: oneshot::Sender<ChannelRouteResult>,
    ) {
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
        let msg = Message {
            tags,
            prefix: Some(slirc_proto::Prefix::Nickname(
                user_context.nickname.clone(),
                user_context.username.clone(),
                user_context.hostname.clone(),
            )),
            command: if is_notice {
                Command::NOTICE(self.name.clone(), text)
            } else {
                Command::PRIVMSG(self.name.clone(), text)
            },
        };

        let msg_arc = Arc::new(msg);
        for (uid, sender) in &self.senders {
            if uid == &sender_uid {
                continue;
            }

            if let Some(prefix) = status_prefix {
                if let Some(modes) = self.members.get(uid) {
                    let has_status = match prefix {
                        '@' => modes.op || modes.admin || modes.owner,
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

            let _ = sender.send((*msg_arc).clone()).await;
        }

        let _ = reply_tx.send(ChannelRouteResult::Sent);
    }
}
