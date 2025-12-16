//! List mode handling (ban, except, invite lists).

use crate::handlers::{Context, HandlerError, HandlerResult, server_reply};
use crate::state::RegisteredState;
use slirc_proto::{ChannelMode, Mode, Response};

/// Check if this is a list mode query (Type A mode with no argument).
/// Returns the list mode type if it's a query, None otherwise.
pub(super) fn get_list_mode_query(modes: &[Mode<ChannelMode>]) -> Option<ChannelMode> {
    if modes.len() == 1 && modes[0].arg().is_none() {
        let mode_type = modes[0].mode();
        // Type A (list) modes: Ban, Exception, InviteException, Quiet
        if matches!(
            mode_type,
            ChannelMode::Ban
                | ChannelMode::Exception
                | ChannelMode::InviteException
                | ChannelMode::Quiet
        ) {
            return Some(mode_type.clone());
        }
    }
    None
}

/// Send a list mode's entries for a channel.
pub(super) async fn send_list_mode(
    ctx: &mut Context<'_, RegisteredState>,
    channel_lower: &str,
    canonical_name: &str,
    list_mode: ChannelMode,
) -> HandlerResult {
    let nick = ctx.nick();

    if let Some(channel) = ctx.matrix.channels.get(channel_lower) {
        // Get the appropriate list and response codes based on mode type
        let (mode_char, reply_code, end_code, end_msg) = match list_mode {
            ChannelMode::Ban => (
                'b',
                Response::RPL_BANLIST,
                Response::RPL_ENDOFBANLIST,
                "End of channel ban list",
            ),
            ChannelMode::Exception => (
                'e',
                Response::RPL_EXCEPTLIST,
                Response::RPL_ENDOFEXCEPTLIST,
                "End of channel exception list",
            ),
            ChannelMode::InviteException => (
                'I',
                Response::RPL_INVITELIST,
                Response::RPL_ENDOFINVITELIST,
                "End of channel invite exception list",
            ),
            ChannelMode::Quiet => (
                'q',
                Response::RPL_QUIETLIST,
                Response::RPL_ENDOFQUIETLIST,
                "End of channel quiet list",
            ),
            _ => return Ok(()),
        };

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if (channel
            .send(crate::state::actor::ChannelEvent::GetList {
                mode: mode_char,
                reply_tx,
            })
            .await)
            .is_err()
        {
            return Err(HandlerError::Internal("Channel actor died".to_string()));
        }

        let list = match reply_rx.await {
            Ok(l) => l,
            Err(_) => return Err(HandlerError::Internal("Channel actor died".to_string())),
        };

        for entry in list {
            let reply = server_reply(
                ctx.server_name(),
                reply_code,
                vec![
                    nick.to_string(),
                    canonical_name.to_string(),
                    entry.mask.clone(),
                    entry.set_by.clone(),
                    entry.set_at.to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        let end_reply = server_reply(
            ctx.server_name(),
            end_code,
            vec![
                nick.to_string(),
                canonical_name.to_string(),
                end_msg.to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;
    }

    Ok(())
}
