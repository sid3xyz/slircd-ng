//! Server administration commands: DIE, REHASH, RESTART.
//!
//! These commands require operator privileges and use capability-based
//! authorization (Innovation 4) for access control.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, get_nick_or_star, server_notice,
    server_reply,
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
                let reply = Response::err_noprivileges(&nick).with_prefix(ctx.server_prefix());
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

        ctx.matrix
            .lifecycle_manager
            .shutdown_tx
            .send(())
            .map_err(|_| {
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
                let reply = Response::err_noprivileges(&nick).with_prefix(ctx.server_prefix());
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

        // Get the config path stored in Matrix (set at startup from command line arg)
        let config_path = ctx.matrix.config_path.clone();

        let reload_result = async {
            // Phase 1: Load and validate new configuration from disk
            let new_config = crate::config::Config::load(&config_path)
                .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;

            tracing::debug!("New config loaded and validated");

            // Phase 2: Reload ban lists from database (always safe)
            let dlines = ctx.db.bans().get_active_dlines().await?;
            let zlines = ctx.db.bans().get_active_zlines().await?;

            // Phase 3: Update IP deny list with fresh bans
            match ctx.matrix.security_manager.ip_deny_list.write() {
                Ok(mut deny_list) => {
                    deny_list.reload_from_database(&dlines, &zlines);
                    tracing::debug!("IP deny list reloaded from database");
                }
                Err(e) => {
                    anyhow::bail!("Failed to acquire write lock on IP deny list: {}", e)
                }
            }

            // Phase 4: Atomically swap hot-reloadable configuration
            // This is the key innovation: using parking_lot::RwLock for atomic swaps
            {
                let new_hot_config = crate::state::HotConfig::from_config(&new_config);
                let mut hot_config = ctx.matrix.hot_config.write();
                *hot_config = new_hot_config;
                tracing::debug!(
                    "Hot config atomically swapped: description='{}', opers={}",
                    hot_config.description,
                    hot_config.oper_blocks.len()
                );
            }

            tracing::info!(
                oper_count = %new_config.oper.len(),
                "Configuration reloaded successfully"
            );

            Ok::<_, anyhow::Error>(())
        }
        .await;

        match reload_result {
            Ok(()) => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        "REHASH complete: Configuration reloaded (IP bans, server info, operators)",
                    ))
                    .await?;
                tracing::info!(oper = %nick, "REHASH completed successfully");
            }
            Err(e) => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        format!("REHASH failed (config not updated): {}", e),
                    ))
                    .await?;
                tracing::warn!(oper = %nick, error = %e, "REHASH failed - original config preserved");
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
                let reply = Response::err_noprivileges(&nick).with_prefix(ctx.server_prefix());
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

        ctx.matrix
            .lifecycle_manager
            .shutdown_tx
            .send(())
            .map_err(|_| {
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
