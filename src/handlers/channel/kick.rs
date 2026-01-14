//! KICK command handler.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, resolve_nick_or_nosuchnick,
    user_mask_from_state,
};
use super::common::{
    build_kick_pairs, kick_reason_or_default, parse_channel_list, parse_nick_list,
};
use crate::state::RegisteredState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for KICK command.
///
/// Uses capability-based authorization (Innovation 4).
/// # RFC 2812 §3.2.8
///
/// Kick command - Requests forced removal of a user from a channel.
///
/// **Specification:** [RFC 2812 §3.2.8](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.8)
///
/// **Compliance:** 5/7 irctest pass
pub struct KickHandler;

#[async_trait]
impl PostRegHandler for KickHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let kicker_nick = &ctx.state.nick;

        // KICK <channel[,channel2,...]> <nick[,nick2,...]> [reason]
        let channels_arg = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let targets_arg = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        // RFC2812: default comment is the nickname of the user issuing the KICK
        let reason_str = kick_reason_or_default(msg.arg(2), kicker_nick).to_string();

        if channels_arg.is_empty() || targets_arg.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Parse comma-separated channels and targets using shared utilities
        let channel_names = parse_channel_list(channels_arg);
        let target_nicks = parse_nick_list(targets_arg);

        // Build channel:target pairs using RFC 2812 rules
        let pairs = build_kick_pairs(&channel_names, &target_nicks);

        for (channel_name, target_nick) in pairs {
            let channel_lower = irc_to_lower(channel_name);

            // Get channel
            let channel_tx = match ctx.require_channel_exists(channel_name) {
                Ok(tx) => tx,
                Err(e) => {
                    if let Some(msg) = e.to_irc_reply(ctx.server_name(), &nick, "KICK") {
                        ctx.sender.send(msg).await?;
                    }
                    crate::metrics::record_command_error("KICK", e.error_code());
                    continue;
                }
            };

            // Find target user
            let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "KICK", target_nick).await?
            else {
                continue;
            };

            // Request KICK capability from authority (Innovation 4)
            let authority = ctx.authority();
            let kick_cap = authority.request_kick_cap(ctx.uid, channel_name).await;

            // If capability granted, pass it to actor.
            // The actor will verify either the capability OR internal op status.
            let (reply_tx, reply_rx) = oneshot::channel();
            let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user.clone(), host.clone());

            let event = ChannelEvent::Kick {
                params: crate::state::actor::KickParams {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix,
                    target_uid: target_uid.clone(),
                    target_nick: target_nick.to_string(),
                    reason: reason_str.clone(),
                    force: false, // Deprecated in favor of cap, but kept for internal use
                    cap: kick_cap,
                },
                reply_tx,
            };

            if (channel_tx.send(event).await).is_err() {
                continue;
            }

            match reply_rx.await {
                Ok(Ok(())) => {
                    // Success.
                    // We also need to remove channel from target's user struct.
                    let account =
                        if let Some(user_ref) = ctx.matrix.user_manager.users.get(&target_uid) {
                            let mut user_data = user_ref.write().await;
                            user_data.channels.remove(&channel_lower);
                            user_data.account.clone()
                        } else {
                            None
                        };

                    if ctx.matrix.config.multiclient.enabled
                        && let Some(account) = account
                    {
                        ctx.matrix
                            .client_manager
                            .record_channel_part(&account, &channel_lower)
                            .await;
                    }

                    info!(
                        kicker = %nick,
                        target = %target_nick,
                        channel = %channel_name,
                        "User kicked from channel"
                    );
                }
                Ok(Err(e)) => {
                    let reply = e.to_irc_reply(ctx.server_name(), &nick, channel_name);
                    ctx.sender.send(reply).await?;
                }
                Err(_) => {}
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::common::{
        build_kick_pairs, kick_reason_or_default, parse_channel_list, parse_nick_list,
    };
    use slirc_proto::irc_to_lower;

    // ========================================================================
    // KICK-specific integration tests
    // Tests the combination of parsing + pairing logic used by KICK
    // ========================================================================

    #[test]
    fn test_kick_single_target() {
        let channels = parse_channel_list("#test");
        let targets = parse_nick_list("victim");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], ("#test", "victim"));
    }

    #[test]
    fn test_kick_multiple_targets_single_channel() {
        let channels = parse_channel_list("#test");
        let targets = parse_nick_list("alice,bob,charlie");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs.len(), 3);
        assert_eq!(
            pairs,
            vec![("#test", "alice"), ("#test", "bob"), ("#test", "charlie")]
        );
    }

    #[test]
    fn test_kick_paired_channels_and_targets() {
        let channels = parse_channel_list("#foo,#bar");
        let targets = parse_nick_list("alice,bob");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs, vec![("#foo", "alice"), ("#bar", "bob")]);
    }

    #[test]
    fn test_kick_with_reason() {
        let reason = kick_reason_or_default(Some("spamming the channel"), "kicker");
        assert_eq!(reason, "spamming the channel");
    }

    #[test]
    fn test_kick_without_reason_uses_kicker_nick() {
        let reason = kick_reason_or_default(None, "OperatorNick");
        assert_eq!(reason, "OperatorNick");
    }

    #[test]
    fn test_kick_empty_reason_uses_kicker_nick() {
        let reason = kick_reason_or_default(Some(""), "OperatorNick");
        assert_eq!(reason, "OperatorNick");
    }

    // ========================================================================
    // Channel name case handling for KICK
    // ========================================================================

    #[test]
    fn test_kick_channel_case_preservation() {
        let channels = parse_channel_list("#MyChannel");
        assert_eq!(channels[0], "#MyChannel"); // Preserved for display
        assert_eq!(irc_to_lower(channels[0]), "#mychannel"); // Lowered for lookup
    }

    #[test]
    fn test_kick_target_nick_preservation() {
        let targets = parse_nick_list("CamelCaseNick");
        assert_eq!(targets[0], "CamelCaseNick"); // Preserved
    }

    // ========================================================================
    // Edge cases
    // ========================================================================

    #[test]
    fn test_kick_whitespace_in_lists() {
        let channels = parse_channel_list(" #foo , #bar ");
        let targets = parse_nick_list(" alice , bob ");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs, vec![("#foo", "alice"), ("#bar", "bob")]);
    }

    #[test]
    fn test_kick_empty_entries_filtered() {
        let channels = parse_channel_list("#foo,,#bar");
        let targets = parse_nick_list("alice,,bob");

        // Empty entries are filtered by parse_* functions
        assert_eq!(channels, vec!["#foo", "#bar"]);
        assert_eq!(targets, vec!["alice", "bob"]);
    }

    #[test]
    fn test_kick_reason_with_special_chars() {
        let reason = kick_reason_or_default(Some("Goodbye! 🦀 See RFC 2812"), "kicker");
        assert_eq!(reason, "Goodbye! 🦀 See RFC 2812");
    }

    #[test]
    fn test_kick_mismatched_counts() {
        // 2 channels, 3 targets - only first 2 pairs
        let channels = parse_channel_list("#foo,#bar");
        let targets = parse_nick_list("alice,bob,charlie");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs, vec![("#foo", "alice"), ("#bar", "bob")]);
    }

    #[test]
    fn test_kick_single_channel_many_nicks() {
        // This is the common use case: /kick #channel nick1,nick2,nick3
        let channels = parse_channel_list("#ops");
        let targets = parse_nick_list("spammer1,spammer2,spammer3,spammer4");
        let pairs = build_kick_pairs(&channels, &targets);

        assert_eq!(pairs.len(), 4);
        for (channel, _) in &pairs {
            assert_eq!(*channel, "#ops");
        }
    }
}
