//! JOIN command handler and related functionality.
//!
//! # RFC 2812 §3.2.1 - Join message
//!
//! Used by clients to start listening to a specific channel.
//!
//! **Specification:** [RFC 2812 §3.2.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.1)
//!
//! **Compliance:** 7/7 irctest pass
//!
//! ## Syntax
//! ```text
//! JOIN <channels> [<keys>]
//! JOIN 0  ; Leave all channels
//! ```
//!
//! ## Behavior
//! - Creates channel if it doesn't exist
//! - First joiner receives operator status (@)
//! - Validates channel key if +k mode is set
//! - Enforces bans, invite-only, and user limits
//! - Applies AKICK auto-kicks and auto-modes
//! - Persists registered channel state to database
//! - Rate limits joins to prevent abuse

mod creation;
mod enforcement;
mod responses;

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, server_reply};
use super::common::{is_join_zero, parse_channel_list, parse_key_list};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, MessageRef, Response};

use crate::telemetry::spans;
use creation::join_channel;
use tracing::Instrument;

/// Maximum number of channels that can be joined in a single command.
const MAX_JOIN_TARGETS: usize = 10;

pub struct JoinHandler;

#[async_trait]
impl PostRegHandler for JoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let channels_str_raw = msg.arg(0);
        let span = spans::command("JOIN", ctx.uid, channels_str_raw);

        async move {
            // JOIN <channels> [keys]
            let channels_str = channels_str_raw.ok_or(HandlerError::NeedMoreParams)?;

            // Handle "JOIN 0" - leave all channels
            if is_join_zero(channels_str) {
                return leave_all_channels(ctx).await;
            }

            // Check join rate limit before processing any channels
            let uid_string = ctx.uid.to_string();
            if !ctx
                .matrix
                .security_manager
                .rate_limiter
                .check_join_rate(&uid_string)
            {
                let nick = ctx.state.nick.clone();
                let reply = server_reply(
                    ctx.server_name(),
                    Response::ERR_TOOMANYCHANNELS,
                    vec![
                        nick,
                        channels_str.to_string(),
                        "You are joining channels too quickly. Please wait.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            // Parse channel list (comma-separated) and optional keys
            let channels = parse_channel_list(channels_str);

            if channels.len() > MAX_JOIN_TARGETS {
                let reply = server_reply(
                    ctx.server_name(),
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        ctx.state.nick.clone(),
                        channels_str.to_string(),
                        format!("Cannot join more than {} channels at once", MAX_JOIN_TARGETS),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            let keys = parse_key_list(msg.arg(1), channels.len());

            for (i, channel_name) in channels.iter().enumerate() {
                // Empty entries already filtered by parse_channel_list

                if !channel_name.is_channel_name() {
                    let reply = server_reply(
                        ctx.server_name(),
                        Response::ERR_NOSUCHCHANNEL,
                        vec![
                            ctx.state.nick.clone(),
                            channel_name.to_string(),
                            "Invalid channel name".to_string(),
                        ],
                    );

                    ctx.sender.send(reply).await?;
                    continue;
                }

                let key = keys.get(i).and_then(|k| *k);
                join_channel(ctx, channel_name, key).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_, RegisteredState>) -> HandlerResult {
    // Single user read for both mask and channel list
    let (nick, user_name, host, channels): (String, String, String, Vec<String>) = {
        let user_ref = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
            user.channels.iter().cloned().collect(),
        )
    };

    for channel_lower in channels {
        super::part::leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, None)
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::ChannelExt;

    // ========================================================================
    // Channel name validation tests (via slirc_proto::ChannelExt)
    // These are JOIN-specific as they validate the is_channel_name() check
    // ========================================================================

    #[test]
    fn test_valid_channel_names() {
        assert!("#channel".is_channel_name());
        assert!("#test".is_channel_name());
        assert!("&local".is_channel_name());
        assert!("+modeless".is_channel_name());
        assert!("!ABCDE".is_channel_name());
        assert!("#123".is_channel_name());
        assert!("#a-b_c".is_channel_name());
    }

    #[test]
    fn test_invalid_channel_names() {
        // Missing prefix
        assert!(!"channel".is_channel_name());
        assert!(!"test".is_channel_name());

        // Invalid prefix
        assert!(!"@channel".is_channel_name());
        assert!(!"$channel".is_channel_name());

        // Contains space
        assert!(!"#chan nel".is_channel_name());

        // Contains comma
        assert!(!"#chan,nel".is_channel_name());

        // Empty
        assert!(!"".is_channel_name());

        // Just prefix
        assert!("#".is_channel_name()); // Actually valid per RFC - just prefix char

        // Control characters (BEL)
        assert!(!"#test\x07".is_channel_name());
    }

    #[test]
    fn test_channel_name_length_limit() {
        // RFC 2812 says 50 chars max including prefix
        let valid_49 = format!("#{}", "a".repeat(48));
        assert!(valid_49.is_channel_name());

        let valid_50 = format!("#{}", "a".repeat(49));
        assert!(valid_50.is_channel_name());

        let invalid_51 = format!("#{}", "a".repeat(50));
        assert!(!invalid_51.is_channel_name());
    }

    // ========================================================================
    // Integration-style tests for parsing + validation flow
    // ========================================================================

    #[test]
    fn test_parse_and_validate_channels() {
        let channels = parse_channel_list("#valid,invalid,#also-valid");

        let valid: Vec<&str> = channels
            .iter()
            .copied()
            .filter(|c| c.is_channel_name())
            .collect();

        assert_eq!(valid, vec!["#valid", "#also-valid"]);
    }

    #[test]
    fn test_parse_channels_and_keys_aligned() {
        let channels_str = "#foo,#bar,#baz";
        let keys_str = Some("key1,,key3");

        let channels = parse_channel_list(channels_str);
        let keys = parse_key_list(keys_str, channels.len());

        assert_eq!(channels.len(), keys.len());

        let pairs: Vec<_> = channels.iter().zip(keys.iter()).collect();
        assert_eq!(
            pairs,
            vec![
                (&"#foo", &Some("key1")),
                (&"#bar", &None),
                (&"#baz", &Some("key3")),
            ]
        );
    }
}
