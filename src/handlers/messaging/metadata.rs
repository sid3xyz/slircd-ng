//! METADATA command handler (Ergo extension).
//!
//! The METADATA command allows getting, setting, and listing metadata
//! associated with users and channels.
//!
//! Format:
//! - `METADATA GET <target> <key>` - Get a metadata key for a user or channel
//! - `METADATA SET <target> <key> [value]` - Set a metadata key (empty value deletes)
//! - `METADATA LIST <target>` - List all metadata for a target
//!
//! This handler provides stub implementations that respond with proper IRC
//! replies but do not yet persist metadata. Full implementation requires:
//! - Adding metadata storage to Matrix or user/channel state
//! - Parsing metadata requests and returning key-value pairs
//! - Enforcing proper ownership/permissions

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

pub struct MetadataHandler;

#[async_trait]
impl PostRegHandler for MetadataHandler {
    async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, _msg: &MessageRef<'_>) -> HandlerResult {
        // TODO: Implement full METADATA storage and retrieval
        // For now, respond with appropriate IRC replies

        // METADATA responses should use special numeric 761-769
        // Since those aren't in Response enum yet, use standard replies
        
        let reply = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
            command: Command::Response(
                Response::RPL_HELPSTART,  // Placeholder numeric
                vec![
                    ctx.state.nick.clone(),
                    "METADATA".to_string(),
                    "METADATA command not yet implemented - full storage pending".to_string(),
                ],
            ),
        };
        ctx.sender.send(reply).await?;
        Ok(())
    }
}
