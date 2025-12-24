//! SASL authentication handler.
//!
//! Supports PLAIN and EXTERNAL mechanisms, both pre- and post-registration.
//! Post-registration SASL allows clients to re-authenticate to a different account.

use super::types::{SaslState, SecureString};
use crate::handlers::{notify_extended_monitor_watchers, Context, HandlerResult, UniversalHandler};
use crate::state::{SaslAccess, SessionState};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};
use tracing::{debug, info, warn};
use zeroize::Zeroize;

/// Handler for AUTHENTICATE command (SASL authentication).
///
/// This is a universal handler that works both pre- and post-registration,
/// enabling re-authentication to different accounts after connection.
pub struct AuthenticateHandler;

#[async_trait]
impl<S: SessionState + SaslAccess> UniversalHandler<S> for AuthenticateHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // AUTHENTICATE <data>
        let data = msg.arg(0).unwrap_or("");

        // Get nick using SessionState trait
        let nick = ctx.state.nick_or_star().to_string();

        // Check if SASL is enabled
        if !ctx.state.capabilities().contains("sasl") {
            // SASL not enabled, ignore
            debug!(nick = %nick, "AUTHENTICATE received but SASL not enabled");
            return Ok(());
        }

        // Handle SASL flow - dispatch to state-specific handlers
        match ctx.state.sasl_state().clone() {
            SaslState::None => handle_sasl_init(ctx, &nick, data).await,
            SaslState::WaitingForExternal => handle_sasl_external(ctx, &nick, data).await,
            SaslState::WaitingForData => handle_sasl_plain_data(ctx, &nick, data).await,
            SaslState::Authenticated => {
                // Already authenticated - allow re-authentication by starting fresh
                debug!(nick = %nick, "AUTHENTICATE after authenticated, starting fresh");
                ctx.state.set_sasl_state(SaslState::None);
                handle_sasl_init(ctx, &nick, data).await
            }
        }
    }
}

/// Handle SASL initiation - client sends mechanism name (PLAIN or EXTERNAL).
async fn handle_sasl_init<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    mechanism: &str,
) -> HandlerResult {
    if mechanism.eq_ignore_ascii_case("PLAIN") {
        ctx.state.set_sasl_state(SaslState::WaitingForData);
        // Send empty challenge (AUTHENTICATE +)
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE("+".to_string()),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, "SASL PLAIN: sent challenge");
    } else if mechanism.eq_ignore_ascii_case("EXTERNAL") {
        // EXTERNAL uses TLS client certificate
        if !ctx.state.is_tls() {
            send_sasl_fail(ctx, nick, "EXTERNAL requires TLS connection").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }

        let certfp = match ctx.state.certfp() {
            Some(fp) => fp.to_string(),
            None => {
                send_sasl_fail(ctx, nick, "No client certificate provided").await?;
                ctx.state.set_sasl_state(SaslState::None);
                return Ok(());
            }
        };

        ctx.state.set_sasl_state(SaslState::WaitingForExternal);
        // Send empty challenge
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE("+".to_string()),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, certfp = %certfp, "SASL EXTERNAL: sent challenge");
    } else {
        // Unsupported mechanism
        send_sasl_fail(ctx, nick, "Unsupported mechanism").await?;
        ctx.state.set_sasl_state(SaslState::None);
    }
    Ok(())
}

/// Handle SASL EXTERNAL response (client confirms).
async fn handle_sasl_external<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // EXTERNAL data is usually empty (+) or authzid. We ignore authzid for now and use certfp.
    // We already verified certfp exists in handle_sasl_init.
    let certfp = ctx.state.certfp().unwrap().to_string();

    // Authenticate using CertFP
    match ctx.db.accounts().find_by_certfp(&certfp).await {
        Ok(Some(account)) => {
            info!(nick = %nick, account = %account.name, "SASL EXTERNAL authentication successful");
            let account_name = account.name.clone();
            send_sasl_success(ctx, nick, &account_name).await?;
            ctx.state.set_sasl_state(SaslState::Authenticated);
            ctx.state.set_account(Some(account.name));

            // Broadcast account change if post-registration
            if ctx.state.is_registered() {
                broadcast_account_change(ctx, nick, &account_name).await;
            }
        }
        Ok(None) => {
            warn!(nick = %nick, certfp = %certfp, "SASL EXTERNAL failed: no account for certfp");
            send_sasl_fail(ctx, nick, "Invalid credentials").await?;
            ctx.state.set_sasl_state(SaslState::None);
        }
        Err(e) => {
            warn!(nick = %nick, certfp = %certfp, error = ?e, "SASL EXTERNAL failed");
            send_sasl_fail(ctx, nick, "Invalid credentials").await?;
            ctx.state.set_sasl_state(SaslState::None);
        }
    }

    Ok(())
}

/// Handle SASL PLAIN data response.
async fn handle_sasl_plain_data<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        ctx.state.sasl_buffer_mut().clear();
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // Accumulate the chunk ("+" alone means empty chunk)
    if data != "+" {
        ctx.state.sasl_buffer_mut().push_str(data);
    }

    // If this chunk is exactly 400 bytes, wait for more
    if data.len() == 400 {
        debug!(nick = %nick, chunk_len = data.len(), total_len = ctx.state.sasl_buffer().len(), "SASL: accumulated chunk, waiting for more");
        return Ok(());
    }

    // We have the complete payload, process it
    let mut full_data = std::mem::take(ctx.state.sasl_buffer_mut());
    debug!(nick = %nick, total_len = full_data.len(), "SASL: processing complete payload");

    // Try to decode and validate
    let result = validate_sasl_plain(&full_data);
    // Zeroize the buffer after decoding (it may contain base64-encoded credentials)
    full_data.zeroize();

    match result {
        Ok((authzid, authcid, password)) => {
            // Validate against database (password is SecureString, zeroized on drop)
            let account_name_ref = if authzid.is_empty() {
                &authcid
            } else {
                &authzid
            };

            match ctx
                .db
                .accounts()
                .identify(account_name_ref, password.as_str())
                .await
            {
                Ok(account) => {
                    info!(nick = %nick, account = %account.name, "SASL PLAIN authentication successful");
                    let account_name = account.name.clone();
                    send_sasl_success(ctx, nick, &account_name).await?;
                    ctx.state.set_sasl_state(SaslState::Authenticated);
                    ctx.state.set_account(Some(account.name));

                    // Broadcast account change if post-registration
                    if ctx.state.is_registered() {
                        broadcast_account_change(ctx, nick, &account_name).await;
                    }
                }
                Err(e) => {
                    warn!(nick = %nick, account = %account_name_ref, error = ?e, "SASL authentication failed");
                    send_sasl_fail(ctx, nick, "Invalid credentials").await?;
                    ctx.state.set_sasl_state(SaslState::None);
                }
            }
        }
        Err(e) => {
            debug!(nick = %nick, error = %e, "SASL PLAIN decode failed");
            send_sasl_fail(ctx, nick, "Invalid SASL credentials").await?;
            ctx.state.set_sasl_state(SaslState::None);
        }
    }
    Ok(())
}

/// Broadcast account change notification after post-registration SASL authentication.
///
/// Sends ACCOUNT message to:
/// - All channels the user is in (for clients with account-notify)
/// - All clients monitoring the user (for clients with extended-monitor + account-notify)
async fn broadcast_account_change<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    account_name: &str,
) {
    // Look up user UID and info
    let nick_lower = slirc_proto::irc_to_lower(nick);
    let (uid, user_info, visible_host, channels) = {
        let Some(uid_ref) = ctx.matrix.user_manager.nicks.get(&nick_lower) else {
            return;
        };
        let uid = uid_ref.clone();
        drop(uid_ref);

        let Some(user_arc_ref) = ctx.matrix.user_manager.users.get(&uid) else {
            return;
        };
        let user_arc = user_arc_ref.clone();
        drop(user_arc_ref);
        let user = user_arc.read().await;
        let user_str = user.user.clone();
        let host = user.visible_host.clone();
        let channels: Vec<String> = user.channels.iter().cloned().collect();
        (uid, user_str, host, channels)
    };

    // Update the account in the user state
    if let Some(user_arc_ref) = ctx.matrix.user_manager.users.get(&uid) {
        let mut user = user_arc_ref.write().await;
        user.account = Some(account_name.to_string());
    }

    // Build ACCOUNT message
    let account_msg = Message {
        tags: None,
        prefix: Some(Prefix::new(nick.to_string(), user_info, visible_host)),
        command: Command::ACCOUNT(account_name.to_string()),
    };

    // Broadcast to all channels user is in
    for channel_name in &channels {
        ctx.matrix
            .channel_manager
            .broadcast_to_channel_with_cap(
                channel_name,
                account_msg.clone(),
                Some(&uid),
                Some("account-notify"),
                None,
            )
            .await;
    }

    // Notify extended-monitor watchers
    notify_extended_monitor_watchers(ctx.matrix, nick, account_msg, "account-notify").await;
}

/// Decode and validate SASL PLAIN credentials.
/// Format: base64(authzid \0 authcid \0 password)
///
/// Returns (authzid, authcid, password) where password is wrapped in SecureString
/// to ensure it is zeroized when dropped.
fn validate_sasl_plain(data: &str) -> Result<(String, String, SecureString), &'static str> {
    // Use slirc_proto's decode_base64 helper
    let mut decoded = slirc_proto::sasl::decode_base64(data).map_err(|_| "Invalid base64")?;

    let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
    if parts.len() != 3 {
        // Zeroize the decoded buffer before returning error
        decoded.zeroize();
        return Err("Invalid SASL PLAIN format");
    }

    let authzid = String::from_utf8(parts[0].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let authcid = String::from_utf8(parts[1].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let password =
        SecureString::new(String::from_utf8(parts[2].to_vec()).map_err(|_| "Invalid UTF-8")?);

    // Zeroize the decoded buffer now that we've extracted what we need
    decoded.zeroize();

    if authcid.is_empty() {
        return Err("Empty authcid");
    }

    Ok((authzid, authcid, password))
}

/// Send SASL success numerics.
async fn send_sasl_success<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    account: &str,
) -> HandlerResult {
    // Build user mask - for registered users we have the actual user/host from matrix
    let mask = if ctx.state.is_registered() {
        // Look up actual user info from matrix
        let nick_lower = slirc_proto::irc_to_lower(nick);
        if let Some(uid_ref) = ctx.matrix.user_manager.nicks.get(&nick_lower) {
            let uid = uid_ref.clone();
            drop(uid_ref);
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
        // Pre-registration: use * for unknown parts
        format!("{}!*@*", nick)
    };

    // RPL_LOGGEDIN (900)
    let reply = Response::rpl_loggedin(nick, &mask, account).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    // RPL_SASLSUCCESS (903)
    let reply = Response::rpl_saslsuccess(nick).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Send SASL failure numerics.
async fn send_sasl_fail<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    _reason: &str,
) -> HandlerResult {
    // ERR_SASLFAIL (904)
    let reply = Response::err_saslfail(nick).with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}
