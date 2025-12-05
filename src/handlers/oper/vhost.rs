use super::super::{
    Context, Handler, HandlerResult, err_needmoreparams, err_nosuchnick, require_oper,
    resolve_nick_to_uid, server_notice,
};
use super::is_valid_hostname;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};

/// Handler for VHOST command.
///
/// `VHOST <nick> <vhost>`
///
/// Sets a virtual hostname for a user (operator only).
/// This updates the user's visible_host field.
pub struct VhostHandler;

#[async_trait]
impl Handler for VhostHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;
        let Ok(oper_nick) = require_oper(ctx).await else {
            return Ok(());
        };

        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &oper_nick, "VHOST"))
                    .await?;
                return Ok(());
            }
        };

        let new_vhost = match msg.arg(1) {
            Some(h) if !h.is_empty() => h,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &oper_nick, "VHOST"))
                    .await?;
                return Ok(());
            }
        };

        if new_vhost.len() > 64 {
            let reply = server_notice(server_name, &oper_nick, "Vhost too long (max 64 chars)");
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        if !is_valid_hostname(new_vhost) {
            let reply = server_notice(
                server_name,
                &oper_nick,
                "Invalid vhost: use alphanumeric, hyphens, dots only",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let target_uid = match resolve_nick_to_uid(ctx, target_nick) {
            Some(uid) => uid,
            None => {
                ctx.sender
                    .send(err_nosuchnick(server_name, &oper_nick, target_nick))
                    .await?;
                return Ok(());
            }
        };

        if let Some(target_user_ref) = ctx.matrix.users.get(&target_uid) {
            let mut target_user = target_user_ref.write().await;
            let old_vhost = target_user.visible_host.clone();
            target_user.visible_host = new_vhost.to_string();

            let reply = server_notice(
                server_name,
                &oper_nick,
                format!(
                    "Changed vhost for {} from {} to {}",
                    target_user.nick, old_vhost, new_vhost
                ),
            );
            ctx.sender.send(reply).await?;

            let channels: Vec<String> = target_user.channels.iter().cloned().collect();
            let target_nick_clone = target_user.nick.clone();
            let target_user_clone = target_user.user.clone();
            drop(target_user);

            let chghost_msg = Message {
                tags: None,
                prefix: Some(Prefix::new(
                    &target_nick_clone,
                    &target_user_clone,
                    &old_vhost,
                )),
                command: Command::CHGHOST(target_user_clone.clone(), new_vhost.to_string()),
            };

            for channel_name in &channels {
                ctx.matrix
                    .broadcast_to_channel_with_cap(
                        channel_name,
                        chghost_msg.clone(),
                        None,
                        Some("chghost"),
                        None,
                    )
                    .await;
            }

            tracing::info!(
                oper = %oper_nick,
                target = %target_nick,
                old_vhost = %old_vhost,
                new_vhost = %new_vhost,
                "VHOST changed"
            );
        }

        Ok(())
    }
}
