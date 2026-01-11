//! RELAYMSG command handler - NOT YET IMPLEMENTED.
//!
//! The RELAYMSG command allows relaying messages between IRC networks.
//! This is an Ergo extension for network bridges and bouncers.
//!
//! Format: `RELAYMSG <relay_from> <target> :<text>`
//!
//! Where:
//! - relay_from: The original sender (network/server/nick format)
//! - target: The destination (channel or user)
//! - text: The message content
//!
//! The relayed message appears with a special prefix indicating the relay source.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

pub struct RelayMsgHandler;

#[async_trait]
impl PostRegHandler for RelayMsgHandler {
    async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, _msg: &MessageRef<'_>) -> HandlerResult {
        // TODO: Implement RELAYMSG command properly
        let reply = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
            command: Command::Response(
                Response::ERR_UNKNOWNCOMMAND,
                vec![ctx.state.nick.clone(), "RELAYMSG".to_string(), "RELAYMSG command not yet implemented".to_string()],
            ),
        };
        ctx.sender.send(reply).await?;
        Ok(())
    }
}
