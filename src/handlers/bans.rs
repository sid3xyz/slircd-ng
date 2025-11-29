//! Ban command handlers.
//!
//! Commands for server bans (operator-only):
//! - KLINE: Ban by nick!user@host mask
//! - DLINE: Ban by IP address
//! - UNKLINE: Remove a K-line
//! - UNDLINE: Remove a D-line

use super::{err_needmoreparams, err_noprivileges, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};

/// Get user's nick and oper status. Returns None if user not found.
async fn get_oper_info(ctx: &Context<'_>) -> Option<(String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.modes.oper))
}

/// Handler for KLINE command.
///
/// `KLINE [time] user@host :reason`
///
/// Bans a user mask from the server.
pub struct KlineHandler;

#[async_trait]
impl Handler for KlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        // KLINE [time] <user@host> <reason>
        // For now, assume first arg is mask, second is reason
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "KLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // TODO: Store K-line in a ban list
        // TODO: Check if any connected users match and disconnect them

        tracing::info!(oper = %nick, mask = %mask, reason = %reason, "KLINE added");

        // Send confirmation
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick, format!("K-line added: {mask} ({reason})")),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for DLINE command.
///
/// `DLINE [time] ip :reason`
///
/// Bans an IP address from the server.
pub struct DlineHandler;

#[async_trait]
impl Handler for DlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        // DLINE [time] <ip> <reason>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "DLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // TODO: Store D-line in a ban list
        // TODO: Check if any connected users match and disconnect them

        tracing::info!(oper = %nick, ip = %ip, reason = %reason, "DLINE added");

        // Send confirmation
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick, format!("D-line added: {ip} ({reason})")),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for UNKLINE command.
///
/// `UNKLINE user@host`
///
/// Removes a K-line.
pub struct UnklineHandler;

#[async_trait]
impl Handler for UnklineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        // UNKLINE <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNKLINE"))
                    .await?;
                return Ok(());
            }
        };

        // TODO: Remove K-line from ban list

        tracing::info!(oper = %nick, mask = %mask, "UNKLINE removed");

        // Send confirmation
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick, format!("K-line removed: {mask}")),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for UNDLINE command.
///
/// `UNDLINE ip`
///
/// Removes a D-line.
pub struct UndlineHandler;

#[async_trait]
impl Handler for UndlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        // UNDLINE <ip>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNDLINE"))
                    .await?;
                return Ok(());
            }
        };

        // TODO: Remove D-line from ban list

        tracing::info!(oper = %nick, ip = %ip, "UNDLINE removed");

        // Send confirmation
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick, format!("D-line removed: {ip}")),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}
