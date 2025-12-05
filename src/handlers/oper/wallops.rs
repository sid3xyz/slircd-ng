use super::super::{
    Context, Handler, HandlerResult, err_needmoreparams, err_noprivileges,
    get_nick_or_star,
};
use super::get_user_full_info;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};

/// Handler for WALLOPS command.
///
/// `WALLOPS :message`
///
/// Sends a message to all users with +w mode (operators).
pub struct WallopsHandler;

#[async_trait]
impl Handler for WallopsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let wallops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "WALLOPS"))
                    .await?;
                return Ok(());
            }
        };

        let Some((sender_nick, sender_user, sender_host, is_oper)) = get_user_full_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender
                .send(err_noprivileges(server_name, &sender_nick))
                .await?;
            return Ok(());
        }

        let wallops_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(sender_nick.clone(), sender_user.clone(), sender_host.clone())),
            command: Command::WALLOPS(wallops_text.to_string()),
        };

        // Always echo WALLOPS back to sender (per Modern IRC spec: servers MAY do this)
        let _ = ctx.sender.send(wallops_msg.clone()).await;

        // Send to all users with +w (wallops) or +o (oper) modes, except the sender
        for user_entry in ctx.matrix.users.iter() {
            let user = user_entry.read().await;
            if user.uid != ctx.uid
                && (user.modes.wallops || user.modes.oper)
                && let Some(sender) = ctx.matrix.senders.get(&user.uid)
            {
                let _ = sender.send(wallops_msg.clone()).await;
            }
        }

        Ok(())
    }
}
