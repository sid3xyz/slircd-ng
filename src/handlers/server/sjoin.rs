use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for the SJOIN command (Safe Join).
///
/// SJOIN is used to sync channel state (modes, topic, members) during bursts
/// and netsplit merges. It handles TS conflict resolution.
pub struct SJoinHandler;

#[async_trait]
impl ServerHandler for SJoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: SJOIN <ts> <channel> <modes> [args...] :<users>

        let ts_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let channel_name = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let modes = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        let ts = ts_str.parse::<u64>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid timestamp: {}", ts_str))
        })?;

        let arg_count = msg.args().len();
        if arg_count < 4 {
             return Err(HandlerError::NeedMoreParams);
        }

        // The last argument is the user list
        let user_list_str = msg.arg(arg_count - 1).unwrap();

        // Arguments between modes and the last argument are mode args
        let mut mode_args = Vec::new();
        for i in 3..(arg_count - 1) {
            if let Some(arg) = msg.arg(i) {
                mode_args.push(arg.to_string());
            }
        }

        // Parse user list
        let mut users = Vec::new();
        for user_token in user_list_str.split_whitespace() {
            let mut prefix = String::new();
            let mut uid = String::new();

            for (i, c) in user_token.char_indices() {
                if c.is_alphanumeric() {
                    uid = user_token[i..].to_string();
                    break;
                } else {
                    prefix.push(c);
                }
            }

            if uid.is_empty() {
                uid = user_token.to_string();
            }

            users.push((prefix, uid));
        }

        // Get or create channel actor
        let tx = ctx.matrix.channel_manager.get_or_create_actor(
            channel_name.to_string(),
            std::sync::Arc::downgrade(ctx.matrix),
        ).await;

        let event = ChannelEvent::SJoin {
            ts,
            modes: modes.to_string(),
            mode_args,
            users,
        };

        if let Err(e) = tx.send(event).await {
            warn!(channel = %channel_name, error = %e, "Failed to send SJOIN to channel actor");
        }

        Ok(())
    }
}
