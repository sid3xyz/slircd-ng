//! NPC command handler (ROLEPLAY extension) - NOT YET IMPLEMENTED.
//!
//! The NPC command allows users to send messages to a channel as a different character/nick.
//! This is part of the ROLEPLAY IRCv3 extension from Ergo.
//!
//! Format: `NPC <channel> <nick> :<text>`
//!
//! The message appears in the channel as if sent by the specified nick (roleplay character),
//! but with a special prefix indicating the actual sender.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

pub struct NpcHandler;

#[async_trait]
impl PostRegHandler for NpcHandler {
    async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, _msg: &MessageRef<'_>) -> HandlerResult {
        // TODO: Implement NPC command properly
        let reply = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
            command: Command::Response(
                Response::ERR_UNKNOWNCOMMAND,
                vec![ctx.state.nick.clone(), "NPC".to_string(), "NPC command not yet implemented".to_string()],
            ),
        };
        ctx.sender.send(reply).await?;
        Ok(())
    }
}

