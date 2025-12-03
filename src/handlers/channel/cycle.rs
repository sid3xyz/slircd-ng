//! CYCLE command handler.
//!
//! Implements the CYCLE command (Part + Join in one command).

use super::super::{Context, Handler, HandlerError, HandlerResult};
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
        let part_message = msg.arg(1).map(|s| s.to_string());

        // Process each channel
        let channels: Vec<&str> = channels_str.split(',').collect();
        
        for channel_name in &channels {
            if channel_name.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);

            // Check if user is in the channel
            if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel_ref.read().await;
                if !channel.is_member(ctx.uid) {
                    continue; // Skip if not in channel
                }
            } else {
                continue; // Channel doesn't exist
            }

            // Part from channel
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let mut user = user_ref.write().await;
                user.channels.remove(&channel_lower);
            }

            if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
                let mut channel = channel_ref.write().await;
                channel.remove_member(ctx.uid);
            }

            // Now use JOIN handler to rejoin
            // This is simpler than duplicating all the JOIN logic
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
