use super::super::{
    Context, Handler, HandlerError, HandlerResult, require_oper, server_notice, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tokio::sync::mpsc;

pub struct DieHandler;

#[async_trait]
impl Handler for DieHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        ctx.sender
            .send(server_notice(
                server_name,
                &nick,
                "Server shutting down by operator request",
            ))
            .await?;

        tracing::warn!(oper = %nick, "DIE command issued - initiating shutdown");

        ctx.matrix.shutdown_tx.send(()).map_err(|_| {
            tracing::error!("Failed to send shutdown signal - no receivers");
            HandlerError::Send(mpsc::error::SendError(slirc_proto::Message::notice(
                "*",
                "Shutdown signal failed",
            )))
        })?;

        Ok(())
    }
}

pub struct RehashHandler;

#[async_trait]
impl Handler for RehashHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        let reply = server_reply(
            server_name,
            Response::RPL_REHASHING,
            vec![
                nick.clone(),
                "config.toml".to_string(),
                "Rehashing".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        let reload_result = async {
            let dlines = ctx.db.bans().get_active_dlines().await?;
            let zlines = ctx.db.bans().get_active_zlines().await?;

            match ctx.matrix.ip_deny_list.write() {
                Ok(mut deny_list) => {
                    deny_list.reload_from_database(&dlines, &zlines);
                    Ok::<_, anyhow::Error>(())
                }
                Err(e) => {
                    anyhow::bail!("Failed to acquire write lock on IP deny list: {}", e)
                }
            }
        }
        .await;

        match reload_result {
            Ok(()) => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        "REHASH complete: IP deny list reloaded from database",
                    ))
                    .await?;
                tracing::info!(oper = %nick, "REHASH completed successfully");
            }
            Err(e) => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        format!("REHASH warning: {}", e),
                    ))
                    .await?;
                tracing::warn!(oper = %nick, error = %e, "REHASH completed with errors");
            }
        }

        Ok(())
    }
}

pub struct RestartHandler;

#[async_trait]
impl Handler for RestartHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        ctx.sender
            .send(server_notice(
                server_name,
                &nick,
                "Server restarting by operator request (exec replacement)",
            ))
            .await?;

        tracing::warn!(oper = %nick, "RESTART command issued - exec restarting");

        ctx.matrix.shutdown_tx.send(()).map_err(|_| {
            tracing::error!("Failed to send shutdown signal - no receivers");
            HandlerError::Send(mpsc::error::SendError(slirc_proto::Message::notice(
                "*",
                "Shutdown signal failed",
            )))
        })?;

        tracing::info!("RESTART: Shutting down (use process supervisor for automatic restart)");

        Ok(())
    }
}
