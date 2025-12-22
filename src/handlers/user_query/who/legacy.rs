use super::common::WhoUserInfo;
use super::search::{search_channel_users, search_mask_users};
use crate::handlers::{Context, HandlerResult, server_reply};
use crate::state::RegisteredState;
use slirc_proto::Response;

/// Execute standard RFC 2812 WHO.
pub async fn execute(
    ctx: &mut Context<'_, RegisteredState>,
    mask_str: &str,
    is_channel: bool,
    operators_only: bool,
    multi_prefix: bool,
) -> HandlerResult {
    let server_name = ctx.server_name().to_string();
    let requester_nick = ctx.state.nick.clone();

    let reply_builder = move |user_info: WhoUserInfo, channel: &str| {
        let mut flags = if user_info.is_away { "G" } else { "H" }.to_string();
        if user_info.is_oper {
            flags.push('*');
        }
        if user_info.is_bot {
            flags.push('B');
        }
        flags.push_str(&user_info.channel_prefixes);

        server_reply(
            &server_name,
            Response::RPL_WHOREPLY,
            vec![
                requester_nick.clone(),
                channel.to_string(),
                user_info.user.to_string(),
                user_info.visible_host.to_string(),
                server_name.clone(),
                user_info.nick.to_string(),
                flags,
                format!("0 {}", user_info.realname),
            ],
        )
    };

    if is_channel {
        search_channel_users(ctx, mask_str, operators_only, multi_prefix, reply_builder).await
    } else {
        search_mask_users(ctx, mask_str, operators_only, multi_prefix, reply_builder).await
    }
}
