//! QUIT handler for terminating client sessions.

use crate::handlers::{Context, HandlerError, HandlerResult, UniversalHandler};
use crate::state::SessionState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};
use tracing::info;

pub struct QuitHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for QuitHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        let quit_msg = msg.arg(0).map(|s| s.to_string());

        info!(
            uid = %ctx.uid,
            nick = ?ctx.state.nick(),
            message = ?quit_msg,
            "Client quit"
        );

        // Send QUIT to this session only (bouncer expects per-session visibility)
        if ctx.state.is_registered() {
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            let quit = if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                Message {
                    tags: None,
                    prefix: Some(Prefix::new(
                        user.nick.clone(),
                        user.user.clone(),
                        user.host.clone(),
                    )),
                    command: Command::QUIT(quit_msg.clone()),
                }
            } else {
                Message {
                    tags: None,
                    prefix: None,
                    command: Command::QUIT(quit_msg.clone()),
                }
            };
            ctx.sender.send(quit).await?;
        }

        // Signal quit by returning Quit error that connection loop will handle
        Err(HandlerError::Quit(quit_msg))
    }
}
