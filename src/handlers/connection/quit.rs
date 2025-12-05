//! QUIT handler for terminating client sessions.

use crate::handlers::{Context, Handler, HandlerError, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::info;

/// Handler for QUIT command.
pub struct QuitHandler;

#[async_trait]
impl Handler for QuitHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let quit_msg = msg.arg(0).map(|s| s.to_string());

        info!(
            uid = %ctx.uid,
            nick = ?ctx.handshake.nick,
            message = ?quit_msg,
            "Client quit"
        );

        // Signal quit by returning Quit error that connection loop will handle
        Err(HandlerError::Quit(quit_msg))
    }
}
