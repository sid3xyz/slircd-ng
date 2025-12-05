//! LIST command handler.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for LIST command.
///
/// `LIST [channels [target]]`
///
/// Lists channels and their topics.
pub struct ListHandler;

#[async_trait]
impl Handler for ListHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // LIST [channels]
        let filter = msg.arg(0);

        // RPL_LISTSTART (321): Channel :Users Name (optional, some clients don't expect it)

        // Iterate channels
        for channel_ref in ctx.matrix.channels.iter() {
            let sender = channel_ref.value();
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = sender.send(crate::state::actor::ChannelEvent::GetInfo {
                requester_uid: Some(ctx.uid.to_string()),
                reply_tx: tx
            }).await;

            let channel = match rx.await {
                Ok(info) => info,
                Err(_) => continue,
            };

            // Skip secret channels unless user is a member
            if channel.modes.contains(&crate::state::actor::ChannelMode::Secret) && !channel.is_member {
                continue;
            }

            // Apply filter if provided
            if let Some(f) = filter
                && !channel.name.eq_ignore_ascii_case(f)
            {
                continue;
            }

            let topic_text = channel
                .topic
                .as_ref()
                .map(|t| t.text.clone())
                .unwrap_or_default();

            // RPL_LIST (322): <channel> <# visible> :<topic>
            let reply = server_reply(
                server_name,
                Response::RPL_LIST,
                vec![
                    nick.clone(),
                    channel.name.clone(),
                    channel.member_count.to_string(),
                    topic_text,
                ],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_LISTEND (323): :End of LIST
        let reply = server_reply(
            server_name,
            Response::RPL_LISTEND,
            vec![nick.clone(), "End of LIST".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
