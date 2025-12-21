//! MODE command handler.
//!
//! Handles both user modes and channel modes using slirc-proto's typed MODE parsing.
//!
//! - User modes: `MODE nick [+/-modes]`
//! - Channel modes: `MODE channel [+/-modes [args...]]`

mod channel;
mod common;
mod user;

pub use channel::format_modes_for_log;

use super::{Context, HandlerError, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use crate::telemetry::spans;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::Instrument;

/// Handler for MODE command.
pub struct ModeHandler;

#[async_trait]
impl PostRegHandler for ModeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let target_raw = msg.arg(0);
        let span = spans::command("MODE", ctx.uid, target_raw);

        async move {
            // MODE <target> [modes [params]]
            let target = target_raw.ok_or(HandlerError::NeedMoreParams)?;

            // Determine if this is a user or channel mode based on target
            if common::is_channel_target(target) {
                // Parse channel modes from args
                let mode_args: Vec<&str> = msg.args().iter().skip(1).copied().collect();
                let modes = common::parse_channel_modes(ctx, &mode_args).await?;
                channel::handle_channel_mode(ctx, target, &modes).await
            } else {
                // Parse user modes from args
                let mode_args: Vec<&str> = msg.args().iter().skip(1).copied().collect();
                let modes = common::parse_user_modes(ctx, &mode_args).await?;
                user::handle_user_mode(ctx, target, &modes).await
            }
        }
        .instrument(span)
        .await
    }
}
