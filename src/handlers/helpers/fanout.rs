//! Account fan-out helpers for multiclient/bouncer.

use crate::handlers::{Context, HandlerResult, user_prefix};
use crate::state::{SessionState, session::SaslAccess};
use slirc_proto::{Command, Message};
use std::future::Future;
use std::sync::Arc;

/// Broadcast a message to all sessions on the same account.
///
/// If `skip_self` is true, the current session (`ctx.uid`) is excluded.
pub fn broadcast_to_account<S>(
    ctx: &Context<'_, S>,
    message: Message,
    skip_self: bool,
) -> impl Future<Output = HandlerResult> + Send
where
    S: SessionState + SaslAccess,
{
    let matrix = Arc::clone(ctx.matrix);
    let uid = ctx.uid.to_string();
    let account = ctx.state.account().map(String::from);

    async move { broadcast_to_account_inner(matrix, uid, account, message, skip_self).await }
}

async fn broadcast_to_account_inner(
    matrix: Arc<crate::state::Matrix>,
    uid: String,
    account: Option<String>,
    message: Message,
    skip_self: bool,
) -> HandlerResult {
    let Some(account) = account else {
        return Ok(());
    };

    let sessions = matrix.client_manager.get_sessions(&account);
    for session in sessions {
        if skip_self && session.uid.as_str() == uid {
            continue;
        }

        let sibling_uid = &session.uid;

        let Some(user_ref) = matrix.user_manager.users.get(sibling_uid) else {
            continue;
        };

        let user = user_ref.read().await;
        let prefix = user_prefix(&user.nick, &user.user, &user.visible_host);

        let command = match &message.command {
            Command::JOIN(channel, Some(_), Some(_)) => {
                let account_name = user.account.as_deref().unwrap_or("*");
                Command::JOIN(
                    channel.clone(),
                    Some(account_name.to_string()),
                    Some(user.realname.clone()),
                )
            }
            Command::JOIN(channel, _, _) => Command::JOIN(channel.clone(), None, None),
            Command::PART(channel, reason) => Command::PART(channel.clone(), reason.clone()),
            _ => message.command.clone(),
        };

        let msg = Message {
            tags: message.tags.clone(),
            prefix: Some(prefix),
            command,
        };

        matrix
            .user_manager
            .send_to_uid(sibling_uid, Arc::new(msg))
            .await;
    }

    Ok(())
}
