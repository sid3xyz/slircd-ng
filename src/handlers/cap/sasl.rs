use super::types::{SaslState, SecureString};
use crate::handlers::{Context, HandlerResult, PreRegHandler};
use crate::state::{SessionState, UnregisteredState};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response};
use tracing::{debug, info, warn};
use zeroize::Zeroize;

/// Handler for AUTHENTICATE command (SASL authentication).
pub struct AuthenticateHandler;

#[async_trait]
impl PreRegHandler for AuthenticateHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // AUTHENTICATE <data>
        let data = msg.arg(0).unwrap_or("");

        // Get nick using SessionState trait
        let nick = ctx.state.nick_or_star().to_string();

        // Check if SASL is enabled
        if !ctx.state.capabilities.contains("sasl") {
            // SASL not enabled, ignore
            debug!(nick = %nick, "AUTHENTICATE received but SASL not enabled");
            return Ok(());
        }

        // Handle SASL flow - dispatch to state-specific handlers
        match ctx.state.sasl_state.clone() {
            SaslState::None => handle_sasl_init(ctx, &nick, data).await,
            SaslState::WaitingForExternal => handle_sasl_external(ctx, &nick, data).await,
            SaslState::WaitingForData => handle_sasl_plain_data(ctx, &nick, data).await,
            SaslState::Authenticated => {
                debug!(nick = %nick, "AUTHENTICATE after already authenticated");
                Ok(())
            }
        }
    }
}

/// Handle SASL initiation - client sends mechanism name (PLAIN or EXTERNAL).
async fn handle_sasl_init(
    ctx: &mut Context<'_, UnregisteredState>,
    nick: &str,
    mechanism: &str,
) -> HandlerResult {
    if mechanism.eq_ignore_ascii_case("PLAIN") {
        ctx.state.sasl_state = SaslState::WaitingForData;
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
        if !ctx.state.is_tls {
            send_sasl_fail(ctx, nick, "EXTERNAL requires TLS connection").await?;
            ctx.state.sasl_state = SaslState::None;
            return Ok(());
        }

        let Some(certfp) = ctx.state.certfp.as_ref() else {
            send_sasl_fail(ctx, nick, "No client certificate provided").await?;
            ctx.state.sasl_state = SaslState::None;
            return Ok(());
        };

        ctx.state.sasl_state = SaslState::WaitingForExternal;
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
        ctx.state.sasl_state = SaslState::None;
    }
    Ok(())
}

/// Handle SASL EXTERNAL response (client confirms).
async fn handle_sasl_external(
    ctx: &mut Context<'_, UnregisteredState>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.sasl_state = SaslState::None;
        return Ok(());
    }

    // EXTERNAL data is usually empty (+) or authzid. We ignore authzid for now and use certfp.
    // We already verified certfp exists in handle_sasl_init.
    let certfp = ctx.state.certfp.as_ref().unwrap();

    // Authenticate using CertFP
    match ctx.db.accounts().find_by_certfp(certfp).await {
        Ok(Some(account)) => {
            info!(nick = %nick, account = %account.name, "SASL EXTERNAL authentication successful");
            let user = ctx.state.user.clone().unwrap_or_else(|| "*".to_string());
            send_sasl_success(ctx, nick, &user, &account.name).await?;
            ctx.state.sasl_state = SaslState::Authenticated;
            ctx.state.account = Some(account.name);
        }
        Ok(None) => {
            warn!(nick = %nick, certfp = %certfp, "SASL EXTERNAL failed: no account for certfp");
            send_sasl_fail(ctx, nick, "Invalid credentials").await?;
            ctx.state.sasl_state = SaslState::None;
        }
        Err(e) => {
            warn!(nick = %nick, certfp = %certfp, error = ?e, "SASL EXTERNAL failed");
            send_sasl_fail(ctx, nick, "Invalid credentials").await?;
            ctx.state.sasl_state = SaslState::None;
        }
    }

    Ok(())
}

/// Handle SASL PLAIN data response.
async fn handle_sasl_plain_data(
    ctx: &mut Context<'_, UnregisteredState>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        ctx.state.sasl_buffer.clear();
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.sasl_state = SaslState::None;
        return Ok(());
    }

    // Accumulate the chunk ("+" alone means empty chunk)
    if data != "+" {
        ctx.state.sasl_buffer.push_str(data);
    }

    // If this chunk is exactly 400 bytes, wait for more
    if data.len() == 400 {
        debug!(nick = %nick, chunk_len = data.len(), total_len = ctx.state.sasl_buffer.len(), "SASL: accumulated chunk, waiting for more");
        return Ok(());
    }

    // We have the complete payload, process it
    let mut full_data = std::mem::take(&mut ctx.state.sasl_buffer);
    debug!(nick = %nick, total_len = full_data.len(), "SASL: processing complete payload");

    // Try to decode and validate
    let result = validate_sasl_plain(&full_data);
    // Zeroize the buffer after decoding (it may contain base64-encoded credentials)
    full_data.zeroize();

    match result {
        Ok((authzid, authcid, password)) => {
            // Validate against database (password is SecureString, zeroized on drop)
            let account_name = if authzid.is_empty() { &authcid } else { &authzid };

            match ctx.db.accounts().identify(account_name, password.as_str()).await {
                Ok(account) => {
                    info!(nick = %nick, account = %account.name, "SASL PLAIN authentication successful");
                    let user = ctx.state.user.clone().unwrap_or_else(|| "*".to_string());
                    send_sasl_success(ctx, nick, &user, &account.name).await?;
                    ctx.state.sasl_state = SaslState::Authenticated;
                    ctx.state.account = Some(account.name);
                }
                Err(e) => {
                    warn!(nick = %nick, account = %account_name, error = ?e, "SASL authentication failed");
                    send_sasl_fail(ctx, nick, "Invalid credentials").await?;
                    ctx.state.sasl_state = SaslState::None;
                }
            }
        }
        Err(e) => {
            debug!(nick = %nick, error = %e, "SASL PLAIN decode failed");
            send_sasl_fail(ctx, nick, "Invalid SASL credentials").await?;
            ctx.state.sasl_state = SaslState::None;
        }
    }
    Ok(())
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
    let password = SecureString::new(
        String::from_utf8(parts[2].to_vec()).map_err(|_| "Invalid UTF-8")?
    );

    // Zeroize the decoded buffer now that we've extracted what we need
    decoded.zeroize();

    if authcid.is_empty() {
        return Err("Empty authcid");
    }

    Ok((authzid, authcid, password))
}

/// Send SASL success numerics.
async fn send_sasl_success(
    ctx: &mut Context<'_, UnregisteredState>,
    nick: &str,
    user: &str,
    account: &str,
) -> HandlerResult {
    // Use effective host (WEBIRC/TLS-aware) for prefix
    let host = ctx
        .state
        .webirc_host
        .clone()
        .or(ctx.state.webirc_ip.clone())
        .unwrap_or_else(|| ctx.remote_addr.ip().to_string());

    let mask = format!("{}!{}@{}", nick, user, host);

    // RPL_LOGGEDIN (900)
    let reply = Response::rpl_loggedin(nick, &mask, account)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    // RPL_SASLSUCCESS (903)
    let reply = Response::rpl_saslsuccess(nick)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Send SASL failure numerics.
async fn send_sasl_fail(ctx: &mut Context<'_, UnregisteredState>, nick: &str, _reason: &str) -> HandlerResult {
    // ERR_SASLFAIL (904)
    let reply = Response::err_saslfail(nick)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}
