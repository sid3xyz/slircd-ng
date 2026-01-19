//! WHO handler for listing users matching a mask.
//!
//! Supports both standard WHO (RFC 2812) and WHOX (IRCv3) extensions.

mod common;
mod legacy;
mod search;
pub mod v3;

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply, with_label};
use crate::state::RegisteredState;
use async_trait::async_trait;
use common::WhoxFields;
use slirc_proto::{ChannelExt, MessageRef, Response};

/// Handler for WHO command.
///
/// `WHO <mask> [%<fields>[,<token>]]`
///
/// Returns information about users matching the mask.
/// Supports WHOX extensions when %fields are specified.
///
/// **Specification:** [RFC 2812 §3.6.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.6.1)
/// **Extension:** [IRCv3 WHOX](https://ircv3.net/specs/extensions/whox)
///
/// **Compliance:** 38/39 irctest pass
pub struct WhoHandler;

#[async_trait]
impl PostRegHandler for WhoHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let mask = msg.arg(0);
        let second_arg = msg.arg(1);

        // Parse WHOX fields if present, otherwise check for 'o' flag
        let whox = second_arg.and_then(WhoxFields::parse);

        let operators_only = if whox.is_none() {
            second_arg
                .map(|s| s.eq_ignore_ascii_case("o"))
                .unwrap_or(false)
        } else {
            false // WHOX doesn't use 'o' flag
        };

        let nick = ctx.state.nick.clone();

        // Check if the user has multi-prefix CAP enabled
        let multi_prefix = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone())
            .map(|arc| {
                arc.try_read()
                    .map(|u| u.caps.contains("multi-prefix"))
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        // Determine query type
        if let Some(mask_str) = mask {
            let is_channel = mask_str.is_channel_name();

            if let Some(fields) = whox {
                v3::execute(
                    ctx,
                    mask_str,
                    is_channel,
                    operators_only,
                    multi_prefix,
                    &fields,
                )
                .await?;
            } else {
                legacy::execute(ctx, mask_str, is_channel, operators_only, multi_prefix).await?;
            }
        }
        // No mask = return all visible users (typically empty for privacy)

        // RPL_ENDOFWHO (315) - attach label for labeled-response
        let end_mask = mask
            .map(|s| s.to_string())
            .unwrap_or_else(|| "*".to_string());
        let server_name = ctx.server_name();
        let reply = with_label(
            server_reply(
                server_name,
                Response::RPL_ENDOFWHO,
                vec![nick, end_mask, "End of WHO list".to_string()],
            ),
            ctx.label.as_deref(),
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
