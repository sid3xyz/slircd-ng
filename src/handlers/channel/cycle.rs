//! CYCLE command handler.
//!
//! Implements the CYCLE command (Part + Join in one command).

use super::super::{Context, Handler, HandlerError, HandlerResult, user_mask_from_state};
use super::part::leave_channel_internal;
use async_trait::async_trait;
use slirc_proto::{MessageRef, irc_to_lower};

/// Handler for CYCLE command.
///
/// `CYCLE <channel> [message]`
///
/// Cycles (parts and immediately rejoins) a channel.
/// This is equivalent to sending PART followed by JOIN.
pub struct CycleHandler;

#[async_trait]
impl Handler for CycleHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // CYCLE <channel> [message]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let part_message = msg.arg(1); // Optional part message

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Process each channel
        let channels: Vec<&str> = channels_str.split(',').collect();

        for channel_name in &channels {
            if channel_name.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);

            // Use leave_channel_internal to properly broadcast PART
            leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, part_message)
                .await?;
        }

        // After parting all channels, rejoin them using the original channels_str
        // Create a temporary JOIN command
        use slirc_proto::Command;
        let join_cmd = Command::JOIN(channels_str.to_string(), None, None);
        let join_msg = slirc_proto::Message {
            tags: None,
            prefix: None,
            command: join_cmd,
        };

        // Send JOIN to ourselves via the sender
        ctx.sender.send(join_msg).await?;

        Ok(())
    }
}
