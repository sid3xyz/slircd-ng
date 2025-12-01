//! NAMES command handler.

use super::super::{Context, Handler, HandlerError, HandlerResult, server_reply};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for NAMES command.
pub struct NamesHandler;

#[async_trait]
impl Handler for NamesHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // NAMES [channel [target]]
        let channel_name = msg.arg(0).unwrap_or("");

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        if channel_name.is_empty() {
            // NAMES without channel - not implemented
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    "*".to_string(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(channel_name);

        if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
            let channel = channel.read().await;
            let mut names_list = Vec::new();

            for (uid, member_modes) in &channel.members {
                if let Some(user) = ctx.matrix.users.get(uid) {
                    let user = user.read().await;
                    let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                        format!("{}{}", prefix, user.nick)
                    } else {
                        user.nick.clone()
                    };
                    names_list.push(nick_with_prefix);
                }
            }

            let names_reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_NAMREPLY,
                vec![
                    nick.to_string(),
                    "=".to_string(),
                    channel.name.clone(),
                    names_list.join(" "),
                ],
            );
            ctx.sender.send(names_reply).await?;

            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel.name.clone(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        } else {
            let end_names = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_ENDOFNAMES,
                vec![
                    nick.to_string(),
                    channel_name.to_string(),
                    "End of /NAMES list".to_string(),
                ],
            );
            ctx.sender.send(end_names).await?;
        }

        Ok(())
    }
}
