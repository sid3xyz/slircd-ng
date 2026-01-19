use crate::handlers::{Context, HandlerResult, notify_extended_monitor_watchers};
use crate::state::{SaslAccess, SessionState};
use crate::state::client::DeviceId;
use slirc_proto::{Command, Message, Prefix, Response};
use tracing::{debug, warn};
use crate::state::dashmap_ext::DashMapExt;


/// Extract device ID from SASL username.
///
/// SASL usernames can be in the format `account@device` where `device` is
/// used as the device identifier for bouncer/multiclient functionality.
pub(crate) fn extract_device_id(username: &str) -> (String, Option<DeviceId>) {
    if let Some(at_pos) = username.rfind('@') {
        let account = username[..at_pos].to_string();
        let device = &username[at_pos + 1..];
        if device.is_empty() {
            (account, None)
        } else {
            (account, Some(device.to_string()))
        }
    } else {
        (username.to_string(), None)
    }
}

/// Attach session to client manager after successful SASL authentication.
pub(crate) async fn attach_session_to_client<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    account: &str,
    device_id: Option<DeviceId>,
) {
    // Skip if multiclient is disabled
    if !ctx.matrix.config.multiclient.enabled {
        return;
    }

    // Get session_id from the session state
    let session_id = ctx.state.session_id();

    // Get nick for client tracking
    let nick = ctx.state.nick_or_star();

    // For now, we use an empty IP string
    let ip = String::new();

    // Check policies for this account
    let override_opt = ctx.matrix.client_manager.get_multiclient_override(account);
    let multiclient_allowed = ctx
        .matrix
        .config
        .multiclient
        .is_multiclient_enabled(override_opt);
    let always_on_enabled = ctx.matrix.config.multiclient.is_always_on_enabled(None);
    let auto_away_enabled = ctx.matrix.config.multiclient.is_auto_away_enabled(None);

    // Attach to client manager
    let request = crate::state::managers::client::AttachSessionRequest {
        account,
        nick,
        uid: ctx.uid,
        session_id,
        device_id: device_id.clone(),
        ip,
        multiclient_allowed,
        always_on_enabled,
        auto_away_enabled,
    };

    let result = ctx.matrix.client_manager.attach_session(request).await;

    match &result {
        crate::state::managers::client::AttachResult::Created => {
            debug!(
                account = %account,
                session_id = %session_id,
                device = ?device_id,
                "Created new client for account"
            );
        }
        crate::state::managers::client::AttachResult::Attached {
            reattach,
            first_session,
        } => {
            debug!(
                account = %account,
                session_id = %session_id,
                device = ?device_id,
                reattach = %reattach,
                first_session = %first_session,
                "Attached session to existing client"
            );

            // If multiclient is enabled, prepare autoreplay logic
            if let Some(client_arc) = ctx.matrix.client_manager.get_client(account) {
                let client = client_arc.read().await;
                let existing_uid = ctx.matrix.client_manager.get_existing_uid(account);
                let channels: Vec<(String, crate::state::ChannelMembership)> = client
                    .channels
                    .iter()
                    .map(|(name, membership)| (name.clone(), membership.clone()))
                    .collect();
                let replay_since = device_id.as_ref().and_then(|dev| client.get_last_seen(dev));

                let reattach_info = crate::state::ReattachInfo {
                    account: account.to_string(),
                    device_id: device_id.clone(),
                    channels,
                    replay_since,
                    existing_uid,
                };

                ctx.state.set_reattach_info(Some(reattach_info));
            }
        }
        crate::state::managers::client::AttachResult::MulticlientNotAllowed => {
            warn!(account = %account, "Multiclient not allowed for account");
        }
        crate::state::managers::client::AttachResult::TooManySessions => {
            warn!(account = %account, "Too many sessions for account");
        }
    }

    if device_id.is_some() {
        ctx.state.set_device_id(device_id);
    }
}

/// Base64 encode data for SASL responses.
pub(crate) fn encode_base64(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

/// Send SASL success numerics.
pub(crate) async fn send_sasl_success<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    account: &str,
) -> HandlerResult {
    let mask = if ctx.state.is_registered() {
        let nick_lower = slirc_proto::irc_to_lower(nick);
        if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) {
            if let Some(user_arc_ref) = ctx.matrix.user_manager.users.get(&uid) {
                let user_arc = user_arc_ref.clone();
                drop(user_arc_ref);
                let user = user_arc.read().await;
                format!("{}!{}@{}", nick, user.user, user.visible_host)
            } else {
                format!("{}!*@*", nick)
            }
        } else {
            format!("{}!*@*", nick)
        }
    } else {
        format!("{}!*@*", nick)
    };

    let reply = Response::rpl_loggedin(nick, &mask, account).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    let reply = Response::rpl_saslsuccess(nick).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Send SASL failure numerics.
pub(crate) async fn send_sasl_fail<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    _reason: &str,
) -> HandlerResult {
    let reply = Response::err_saslfail(nick).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Broadcast account change notification.
pub(crate) async fn broadcast_account_change<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    account_name: &str,
) {
    let nick_lower = slirc_proto::irc_to_lower(nick);
    let (uid, user_info, visible_host, channels) = {
        let Some(uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) else { return; };
        let Some(user_arc_ref) = ctx.matrix.user_manager.users.get(&uid) else { return; };
        let user_arc = user_arc_ref.clone();
        drop(user_arc_ref);
        let user = user_arc.read().await;
        (uid, user.user.clone(), user.visible_host.clone(), user.channels.iter().cloned().collect::<Vec<_>>())
    };

    if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(&uid) {
        let mut user = user_arc.write().await;
        user.account = Some(account_name.to_string());
    }

    let account_msg = Message {
        tags: None,
        prefix: Some(Prefix::new(nick.to_string(), user_info, visible_host)),
        command: Command::ACCOUNT(account_name.to_string()),
    };

    for channel_name in &channels {
        ctx.matrix.channel_manager.broadcast_to_channel_with_cap(
            channel_name, account_msg.clone(), Some(&uid), Some("account-notify"), None
        ).await;
    }

    notify_extended_monitor_watchers(ctx.matrix, nick, account_msg, "account-notify").await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_device_id_no_device() {
        let (account, device) = extract_device_id("alice");
        assert_eq!(account, "alice");
        assert!(device.is_none());
    }

    #[test]
    fn test_extract_device_id_with_device() {
        let (account, device) = extract_device_id("alice@phone");
        assert_eq!(account, "alice");
        assert_eq!(device, Some("phone".to_string()));
    }

    #[test]
    fn test_extract_device_id_empty_device() {
        let (account, device) = extract_device_id("alice@");
        assert_eq!(account, "alice");
        assert!(device.is_none());
    }

    #[test]
    fn test_extract_device_id_multiple_at_signs() {
        // Uses last @ as separator
        let (account, device) = extract_device_id("alice@foo@phone");
        assert_eq!(account, "alice@foo");
        assert_eq!(device, Some("phone".to_string()));
    }

    #[test]
    fn test_extract_device_id_empty_account() {
        let (account, device) = extract_device_id("@phone");
        assert_eq!(account, "");
        assert_eq!(device, Some("phone".to_string()));
    }

    #[test]
    fn test_extract_device_id_complex_device_name() {
        let (account, device) = extract_device_id("alice@my-iphone-12");
        assert_eq!(account, "alice");
        assert_eq!(device, Some("my-iphone-12".to_string()));
    }
}
