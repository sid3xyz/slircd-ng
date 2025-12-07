//! SHUN command handlers.
//!
//! SHUN silently ignores commands from matching users without disconnecting them.
//! This is less disruptive than traditional bans and useful for dealing with
//! automated abuse.

use crate::caps::CapabilityAuthority;
use crate::db::Shun;
use crate::handlers::{
    Context, Handler, HandlerResult, err_needmoreparams, err_noprivileges, get_nick_or_star,
    server_notice,
};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for SHUN command.
///
/// `SHUN [time] <mask> [reason]`
///
/// Silently ignores commands from matching users without disconnecting them.
pub struct ShunHandler;

#[async_trait]
impl Handler for ShunHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check capability
        let nick = get_nick_or_star(ctx).await;
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_shun_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, &nick))
                .await?;
            return Ok(());
        };

        // SHUN [time] <mask> [reason]
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "SHUN"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("Shunned");

        // Store shun in database
        if let Err(e) = ctx
            .db
            .bans()
            .add_shun(mask, Some(reason), &nick, None)
            .await
        {
            tracing::error!(error = %e, "Failed to add shun to database");
        } else {
            // Also add to in-memory cache for fast lookup
            let now = chrono::Utc::now().timestamp();
            ctx.matrix.shuns.insert(
                mask.to_string(),
                Shun {
                    mask: mask.to_string(),
                    reason: Some(reason.to_string()),
                    set_by: nick.clone(),
                    set_at: now,
                    expires_at: None,
                },
            );
        }

        tracing::info!(
            oper = %nick,
            mask = %mask,
            reason = %reason,
            "SHUN added"
        );

        // Send confirmation
        ctx.sender
            .send(server_notice(
                server_name,
                &nick,
                format!("Shun added: {mask} ({reason})"),
            ))
            .await?;

        Ok(())
    }
}

/// Handler for UNSHUN command.
///
/// `UNSHUN <mask>`
///
/// Removes a shun.
pub struct UnshunHandler;

#[async_trait]
impl Handler for UnshunHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check capability
        let nick = get_nick_or_star(ctx).await;
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_shun_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, &nick))
                .await?;
            return Ok(());
        };

        // UNSHUN <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNSHUN"))
                    .await?;
                return Ok(());
            }
        };

        // Remove shun from database
        let removed = match ctx.db.bans().remove_shun(mask).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove shun from database");
                false
            }
        };

        if removed {
            // Also remove from in-memory cache
            ctx.matrix.shuns.remove(mask);
            tracing::info!(oper = %nick, mask = %mask, "UNSHUN removed");
        }

        // Send confirmation
        let text = if removed {
            format!("Shun removed: {mask}")
        } else {
            format!("No shun found for: {mask}")
        };
        ctx.sender
            .send(server_notice(server_name, &nick, &text))
            .await?;

        Ok(())
    }
}
