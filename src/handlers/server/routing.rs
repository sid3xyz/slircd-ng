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

        // 1. Is the target local?
        if let Some(sender) = ctx.matrix.user_manager.get_first_sender(target_uid) {
            // Local delivery!
            crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                .with_label_values(&[source_sid, target_sid, "success"])
                .inc();

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
                        crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                            .with_label_values(&[source_sid, target_sid.as_str(), "forwarded"])
                            .inc();
                    } else {
                        warn!(to = %target_uid, "No route to host for routed message");
                        crate::metrics::DISTRIBUTED_MESSAGES_ROUTED
                            .with_label_values(&[source_sid, target_sid.as_str(), "no_route"])
                            .inc();
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
