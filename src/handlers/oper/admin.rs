use super::super::{Context,
    HandlerError, HandlerResult, PostRegHandler, get_nick_or_star,
    server_notice, server_reply,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tokio::sync::mpsc;

/// Handler for DIE command. Uses capability-based authorization (Innovation 4).
pub struct DieHandler;

#[async_trait]
impl PostRegHandler for DieHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = get_nick_or_star(ctx).await;

        // Request DIE capability from authority (Innovation 4)
        let authority = ctx.authority();
        let _die_cap = match authority.request_die_cap(ctx.uid).await {
            Some(cap) => cap,
            None => {
                let reply = Response::err_noprivileges(&nick)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("DIE", "ERR_NOPRIVILEGES", reply).await?;
                return Ok(());
            }
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

/// Handler for REHASH command. Uses capability-based authorization (Innovation 4).
pub struct RehashHandler;

#[async_trait]
impl PostRegHandler for RehashHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = get_nick_or_star(ctx).await;

        // Request REHASH capability from authority (Innovation 4)
        let authority = ctx.authority();
        let _rehash_cap = match authority.request_rehash_cap(ctx.uid).await {
            Some(cap) => cap,
            None => {
                let reply = Response::err_noprivileges(&nick)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("REHASH", "ERR_NOPRIVILEGES", reply).await?;
                return Ok(());
            }
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

/// Handler for RESTART command. Uses capability-based authorization (Innovation 4).
pub struct RestartHandler;

#[async_trait]
impl PostRegHandler for RestartHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = get_nick_or_star(ctx).await;

        // Request RESTART capability from authority (Innovation 4)
        let authority = ctx.authority();
        let _restart_cap = match authority.request_restart_cap(ctx.uid).await {
            Some(cap) => cap,
            None => {
                let reply = Response::err_noprivileges(&nick)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("RESTART", "ERR_NOPRIVILEGES", reply).await?;
                return Ok(());
            }
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
