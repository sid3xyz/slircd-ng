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

        // Innovation: Handle S2S QUIT gracefully without dropping the link
        if ctx.state.is_registered() && ctx.state.is_server() {
            // S2S QUIT messages target a specific user via prefix
            // We must kill that user locally but keep the link alive
            if let Some(prefix) = &msg.prefix {
                // PrefixRef is a struct, identify the source (UID or Server Name)
                // UIDs are parsed as 'nick', Server Names (with dots) as 'host'
                let identifier = prefix.nick.or(prefix.host).unwrap_or(prefix.raw);

                let reason = quit_msg.as_deref().unwrap_or("Client Quit");

                // Determine if this is a UID or Nick
                // Note: We use None for source server ID effectively letting kill_user auto-detect locality
                if ctx.matrix.user_manager.users.contains_key(identifier) {
                    ctx.matrix
                        .user_manager
                        .kill_user(identifier, reason, None)
                        .await;
                } else {
                    // Just log it, don't error out
                    info!("Received QUIT for unknown user/UID: {}", identifier);
                }
            } else {
                info!("Received QUIT from server {} without prefix", ctx.uid);
            }

            // Do NOT return Err(HandlerError::Quit) - that drops the link!
            return Ok(());
        }

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
