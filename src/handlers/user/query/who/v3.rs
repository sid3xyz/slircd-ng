use super::common::{WhoUserInfo, WhoxFields};
use super::search::{search_channel_users, search_mask_users};
use crate::handlers::{Context, HandlerResult, server_reply};
use crate::state::RegisteredState;
use slirc_proto::Response;

/// Execute IRCv3 WHOX.
pub async fn execute(
    ctx: &mut Context<'_, RegisteredState>,
    mask_str: &str,
    is_channel: bool,
    operators_only: bool,
    multi_prefix: bool,
    fields: &WhoxFields,
) -> HandlerResult {
    let server_name = ctx.server_name().to_string();
    let requester_nick = ctx.state.nick.clone();
    // Clone fields to move into closure
    let fields = fields.clone();

    let reply_builder = move |user_info: WhoUserInfo, channel: &str| {
        let mut params = vec![requester_nick.clone()];

        if fields.token {
            params.push(fields.query_token.clone().unwrap_or_default());
        }
        if fields.channel {
            params.push(channel.to_string());
        }
        if fields.username {
            params.push(user_info.user.to_string());
        }
        if fields.ip {
            // Privacy: don't disclose IP
            params.push("255.255.255.255".to_string());
        }
        if fields.hostname {
            params.push(user_info.visible_host.to_string());
        }
        if fields.server {
            params.push(server_name.clone());
        }
        if fields.nick {
            params.push(user_info.nick.to_string());
        }
        if fields.flags {
            let mut flags = if user_info.is_away { "G" } else { "H" }.to_string();
            if user_info.is_oper {
                flags.push('*');
            }
            if user_info.is_bot {
                flags.push('B');
            }
            flags.push_str(&user_info.channel_prefixes);
            params.push(flags);
        }
        if fields.hopcount {
            params.push("0".to_string());
        }
        if fields.idle {
            params.push("0".to_string()); // We don't track idle time currently
        }
        if fields.account {
            params.push(user_info.account.unwrap_or("0").to_string());
        }
        if fields.oplevel {
            params.push("n/a".to_string()); // We don't support op levels
        }
        if fields.realname {
            params.push(user_info.realname.to_string());
        }

        server_reply(&server_name, Response::RPL_WHOSPCRPL, params)
    };

    if is_channel {
        search_channel_users(ctx, mask_str, operators_only, multi_prefix, reply_builder).await
    } else {
        search_mask_users(ctx, mask_str, operators_only, multi_prefix, reply_builder).await
    }
}
