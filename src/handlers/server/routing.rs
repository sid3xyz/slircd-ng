use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::services::traits::Service;
use crate::state::ServerState;
use crate::state::dashmap_ext::DashMapExt;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Tag};
use std::sync::Arc;
use tracing::{debug, warn};

/// Handler for routed PRIVMSG/NOTICE from other servers.
pub struct RoutedMessageHandler;

#[async_trait]
impl ServerHandler for RoutedMessageHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: :SourceUID PRIVMSG TargetUID :text

        let source_uid = match msg.prefix {
            Some(ref p) => {
                if let Some(nick) = p.nick {
                    nick
                } else if let Some(host) = p.host {
                    // If it's a server prefix, it might be in host
                    host
                } else {
                    return Err(HandlerError::ProtocolError(
                        "Invalid source prefix".to_string(),
                    ));
                }
            }
            None => {
                return Err(HandlerError::ProtocolError(
                    "Missing source prefix".to_string(),
                ));
            }
        };

        let target_uid = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        // Extract source SID for metrics
        let source_sid = if source_uid.len() >= 3 {
            &source_uid[0..3]
        } else {
            "unknown"
        };

        // Extract target SID for metrics
        let target_sid = if target_uid.len() >= 3 {
            &target_uid[0..3]
        } else {
            "unknown"
        };

        tracing::info!(from = %source_uid, to = %target_uid, "Received routed message");
        debug!(from = %source_uid, to = %target_uid, "Routing message");

        // 0. Is the target a channel?
        use slirc_proto::ChannelExt;
        if target_uid.is_channel_name() {
            if let Some(tx) = ctx
                .matrix
                .channel_manager
                .channels
                .get(target_uid)
                .map(|v| v.value().clone())
            {
                // Route to ChannelActor for local fanout
                if let Some(m) = crate::metrics::DISTRIBUTED_MESSAGES_ROUTED.get() {
                    m.with_label_values(&[source_sid, "local", "channel"]).inc();
                }

                // Reconstruct message
                // For channel messages, we preserve the original sender
                // channel actor will handle fanout to local members

                // Parse tags
                let tags = if let Some(tags_str) = msg.tags {
                    let mut parsed_tags = Vec::new();
                    for tag_part in tags_str.split(';') {
                        if tag_part.is_empty() {
                            continue;
                        }
                        let (key, value) = if let Some((k, v)) = tag_part.split_once('=') {
                            (k, Some(v.to_string()))
                        } else {
                            (tag_part, None)
                        };
                        parsed_tags.push(Tag::new(key, value));
                    }
                    Some(parsed_tags)
                } else {
                    None
                };

                // Source Prefix (must be resolved to full mask/server for history/clients)
                // If it's a user UID, resolve it.
                let source_prefix =
                    if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(source_uid) {
                        let user = user_arc.read().await;
                        Prefix::Nickname(
                            user.nick.clone(),
                            user.user.clone(),
                            user.visible_host.clone(),
                        )
                    } else if source_uid.len() == 3 {
                        // Server SID?
                        Prefix::ServerName(source_uid.to_string())
                    } else {
                        Prefix::new_from_str(source_uid)
                    };

                let cmd = match msg.command_name() {
                    "PRIVMSG" => Command::PRIVMSG(target_uid.to_string(), text.to_string()),
                    "NOTICE" => Command::NOTICE(target_uid.to_string(), text.to_string()),
                    _ => return Ok(()),
                };

                let out_msg = Message {
                    tags,
                    prefix: Some(source_prefix),
                    command: cmd,
                };

                // Helper to send to channel
                use crate::security::UserContext;
                use crate::state::actor::ChannelEvent;
                use crate::state::actor::ChannelMessageParams;

                // Construct UserContext
                // If source is a user we know, pull their info.
                // Otherwise default to minimal rights (remote user context)
                let user_context =
                    if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(source_uid) {
                        let user = user_arc.read().await;
                        UserContext {
                            nickname: user.nick.clone(),
                            username: user.user.clone(),
                            realname: user.realname.clone(),
                            hostname: user.visible_host.clone(),
                            account: user.account.clone(),
                            server: ctx.state.name.clone(), // Or their actual server?
                            channels: user.channels.iter().cloned().collect(),
                            is_oper: user.modes.oper,
                            oper_type: user.modes.oper_type.clone(),
                            certificate_fp: user.certfp.clone(),
                            sasl_mechanism: None,
                            is_registered: user.modes.registered,
                            is_tls: user.modes.secure,
                        }
                    } else {
                        // Fallback for unknown remote user or server sender
                        UserContext {
                            nickname: source_uid.to_string(),
                            username: "remote".to_string(),
                            realname: "Remote User".to_string(),
                            hostname: "remote".to_string(),
                            account: None,
                            server: "remote".to_string(),
                            channels: Vec::new(),
                            is_oper: false,
                            oper_type: None,
                            certificate_fp: None,
                            sasl_mechanism: None,
                            is_registered: true,
                            is_tls: false,
                        }
                    };

                let params = Box::new(ChannelMessageParams {
                    sender_uid: source_uid.to_string(),
                    text: text.to_string(),
                    tags: out_msg.tags.clone(),
                    is_notice: matches!(msg.command_name(), "NOTICE"),
                    is_tagmsg: matches!(msg.command_name(), "TAGMSG"),
                    user_context,
                    is_registered: true,
                    is_tls: false,
                    is_bot: false,
                    status_prefix: None,
                    timestamp: None,
                    msgid: None,
                    override_nick: None,
                    relaymsg_sender_nick: None,
                    nanotime: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                });

                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

                if let Err(_e) = tx.send(ChannelEvent::Message { params, reply_tx }).await {
                    debug!("Failed to send routed message to channel {}", target_uid);
                } else {
                    // Wait for routing result (optional, but good for metrics/debugging)
                    match reply_rx.await {
                        Ok(result) => {
                            debug!("Routing result for channel {}: {:?}", target_uid, result);
                        }
                        Err(_) => {
                            debug!("Channel actor dropped reply for routed message");
                        }
                    }
                }
            } else {
                debug!("Received message for unknown channel {}", target_uid);
                // Cannot route to non-existent channel
                if let Some(m) = crate::metrics::DISTRIBUTED_MESSAGES_ROUTED.get() {
                    m.with_label_values(&[source_sid, "local", "channel_not_found"]).inc();
                }
            }
            return Ok(());
        }

        // 1. Is the target local?
        if let Some(sender) = ctx.matrix.user_manager.get_first_sender(target_uid) {
            // Local delivery!
            if let Some(m) = crate::metrics::DISTRIBUTED_MESSAGES_ROUTED.get() {
                m.with_label_values(&[source_sid, target_sid, "success"]).inc();
            }

            // We need to reconstruct the message to look like it came from the user
            // But wait, the client expects :Nick!User@Host PRIVMSG TargetNick :text

            // We need to resolve the source UID to a full mask
            let source_mask =
                if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(source_uid) {
                    let user = user_arc.read().await;
                    Prefix::Nickname(
                        user.nick.clone(),
                        user.user.clone(),
                        user.visible_host.clone(),
                    )
                } else {
                    // Unknown source user? Maybe it's a server message?
                    // Or we haven't received the UID burst yet?
                    warn!("Unknown source UID {} for routed message", source_uid);
                    Prefix::new_from_str(source_uid)
                };

            // Resolve target UID to Nickname for the command
            let target_nick =
                if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(target_uid) {
                    let user = user_arc.read().await;
                    user.nick.clone()
                } else {
                    // Should not happen if we found the sender
                    target_uid.to_string()
                };

            // Parse tags from raw string
            let tags = if let Some(tags_str) = msg.tags {
                let mut parsed_tags = Vec::new();
                for tag_part in tags_str.split(';') {
                    if tag_part.is_empty() {
                        continue;
                    }
                    let (key, value) = if let Some((k, v)) = tag_part.split_once('=') {
                        (k, Some(v.to_string()))
                    } else {
                        (tag_part, None)
                    };
                    parsed_tags.push(Tag::new(key, value));
                }
                Some(parsed_tags)
            } else {
                None
            };

            // Check for x-visible-target tag (Innovation 2: Channel routing)
            let visible_target = tags
                .as_ref()
                .and_then(|t| t.iter().find(|tag| tag.0 == "x-visible-target"))
                .and_then(|tag| tag.1.as_ref())
                .cloned();

            let cmd_target = visible_target.unwrap_or(target_nick);

            let cmd = match msg.command_name() {
                "PRIVMSG" => Command::PRIVMSG(cmd_target, text.to_string()),
                "NOTICE" => Command::NOTICE(cmd_target, text.to_string()),
                _ => return Ok(()),
            };

            let out_msg = Message {
                tags,
                prefix: Some(source_mask),
                command: cmd,
            };

            let _ = sender.send(Arc::new(out_msg)).await;
        } else {
            // 1b. Is this message addressed to a local service pseudoclient?
            if ctx.matrix.service_manager.is_service_uid(target_uid) {
                let source_nick =
                    if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(source_uid) {
                        user_arc.read().await.nick.clone()
                    } else {
                        source_uid.to_string()
                    };

                let service_name = ctx.matrix.service_manager.get_service_name(target_uid);
                debug!(
                    from = %source_uid,
                    to = %target_uid,
                    service = ?service_name,
                    "Handling routed service message"
                );

                let effects = if target_uid == ctx.matrix.service_manager.nickserv_uid {
                    ctx.matrix
                        .service_manager
                        .nickserv
                        .handle(ctx.matrix, source_uid, &source_nick, text)
                        .await
                } else if target_uid == ctx.matrix.service_manager.chanserv_uid {
                    ctx.matrix
                        .service_manager
                        .chanserv
                        .handle(ctx.matrix, source_uid, &source_nick, text)
                        .await
                } else {
                    // Unknown service UID (shouldn't happen if is_service_uid returned true)
                    Vec::new()
                };

                crate::services::apply_effects_no_sender(ctx.matrix, &source_nick, effects).await;
                return Ok(());
            }

            // 2. Target is remote?
            // If we received it, and we are not the target, we should route it forward?
            // But we are a star topology or mesh?
            // For now, assume we only receive messages destined for us (or our users).
            // If we implement full routing, we'd check the routing table again.

            // Check if we need to forward it
            // Get target server ID from UID
            if target_uid.len() >= 3 {
                let sid_prefix = &target_uid[0..3];
                let target_sid = slirc_proto::sync::clock::ServerId::new(sid_prefix.to_string());

                if target_sid == ctx.matrix.server_id {
                    // It was meant for us, but user not found?
                    // Send ERR_NOSUCHNICK back?
                    // S2S usually doesn't send error replies for race conditions to avoid loops.
                    warn!("Received message for unknown local user {}", target_uid);
                } else {
                    // Forwarding logic
                    if let Some(peer) = ctx.matrix.sync_manager.get_next_hop(&target_sid) {
                        // Forward as-is
                        // We need to convert MessageRef to Message to send it
                        // Since MessageRef borrows, we need to parse it into owned Message
                        // Or construct a new Message from MessageRef components

                        // Parse tags
                        let tags = if let Some(tags_str) = msg.tags {
                            let mut parsed_tags = Vec::new();
                            for tag_part in tags_str.split(';') {
                                if tag_part.is_empty() {
                                    continue;
                                }
                                let (key, value) = if let Some((k, v)) = tag_part.split_once('=') {
                                    (k, Some(v.to_string()))
                                } else {
                                    (tag_part, None)
                                };
                                parsed_tags.push(Tag::new(key, value));
                            }
                            Some(parsed_tags)
                        } else {
                            None
                        };

                        // Parse prefix
                        let prefix = msg.prefix.as_ref().map(|p| p.to_owned());

                        // Parse command
                        // This is tricky because CommandRef holds references
                        // We need to reconstruct the owned Command
                        // For PRIVMSG/NOTICE it's easy
                        let cmd = match msg.command_name() {
                            "PRIVMSG" => Command::PRIVMSG(target_uid.to_string(), text.to_string()),
                            "NOTICE" => Command::NOTICE(target_uid.to_string(), text.to_string()),
                            _ => return Ok(()), // Should not happen given handler registration
                        };

                        let out_msg = Message {
                            tags,
                            prefix,
                            command: cmd,
                        };

                        debug!(to = %target_uid, via = %peer.name, "Forwarding routed message");
                        let _ = peer.tx.send(Arc::new(out_msg)).await;
                        if let Some(m) = crate::metrics::DISTRIBUTED_MESSAGES_ROUTED.get() {
                            m.with_label_values(&[source_sid, target_sid.as_str(), "forwarded"]).inc();
                        }
                    } else {
                        warn!(to = %target_uid, "No route to host for routed message");
                        if let Some(m) = crate::metrics::DISTRIBUTED_MESSAGES_ROUTED.get() {
                            m.with_label_values(&[source_sid, target_sid.as_str(), "no_route"]).inc();
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use slirc_proto::Tag;

    #[test]
    fn test_tag_parsing() {
        let tags_str = "time=12345;account=test";
        let mut parsed_tags = Vec::new();
        for tag_part in tags_str.split(';') {
            if tag_part.is_empty() {
                continue;
            }
            let (key, value) = if let Some((k, v)) = tag_part.split_once('=') {
                (k, Some(v.to_string()))
            } else {
                (tag_part, None)
            };
            parsed_tags.push(Tag::new(key, value));
        }

        assert_eq!(parsed_tags.len(), 2);
        assert_eq!(parsed_tags[0].0, "time");
        assert_eq!(parsed_tags[0].1.as_deref(), Some("12345"));
        assert_eq!(parsed_tags[1].0, "account");
        assert_eq!(parsed_tags[1].1.as_deref(), Some("test"));
    }
}
