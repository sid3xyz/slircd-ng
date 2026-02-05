use super::common::{
    attach_session_to_client, broadcast_account_change, extract_device_id, send_sasl_fail,
    send_sasl_success,
};
use crate::handlers::cap::types::{SaslState, SecureString};
use crate::handlers::{Context, HandlerResult};
use crate::state::{SaslAccess, SessionState};
use tracing::{debug, info, warn};
use zeroize::Zeroize;

/// Decode and validate SASL PLAIN credentials.
fn validate_sasl_plain(data: &str) -> Result<(String, String, SecureString), &'static str> {
    let mut decoded = slirc_proto::sasl::decode_base64(data).map_err(|_| "Invalid base64")?;

    let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
    if parts.len() != 3 {
        decoded.zeroize();
        return Err("Invalid SASL PLAIN format");
    }

    let authzid = String::from_utf8(parts[0].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let authcid = String::from_utf8(parts[1].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let password =
        SecureString::new(String::from_utf8(parts[2].to_vec()).map_err(|_| "Invalid UTF-8")?);

    decoded.zeroize();

    if authcid.is_empty() {
        return Err("Empty authcid");
    }

    Ok((authzid, authcid, password))
}

pub(crate) async fn handle_sasl_plain_data<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        ctx.state.sasl_buffer_mut().clear();
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // Maximum size for accumulated SASL buffer (16KB)
    // Prevents memory exhaustion attacks
    const MAX_SASL_BUFFER: usize = 16384;

    if data != "+" {
        if ctx.state.sasl_buffer().len() + data.len() > MAX_SASL_BUFFER {
            ctx.state.sasl_buffer_mut().clear();
            send_sasl_fail(ctx, nick, "SASL payload too large").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
        ctx.state.sasl_buffer_mut().push_str(data);
    }

    if data.len() == 400 {
        debug!(nick = %nick, chunk_len = data.len(), total_len = ctx.state.sasl_buffer().len(), "SASL: accumulated chunk, waiting for more");
        return Ok(());
    }

    let mut full_data = std::mem::take(ctx.state.sasl_buffer_mut());
    debug!(nick = %nick, total_len = full_data.len(), "SASL: processing complete payload");

    let result = validate_sasl_plain(&full_data);
    full_data.zeroize();

    match result {
        Ok((authzid, authcid, password)) => {
            let (account_from_authcid, device_id) = extract_device_id(&authcid);

            let account_name_ref = if authzid.is_empty() {
                &account_from_authcid
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
                    info!(nick = %nick, account = %account.name, device = ?device_id, "SASL PLAIN authentication successful");
                    let account_name = account.name.clone();
                    send_sasl_success(ctx, nick, &account_name).await?;
                    ctx.state.set_sasl_state(SaslState::Authenticated);
                    ctx.state.set_account(Some(account.name.clone()));

                    attach_session_to_client(ctx, &account.name, device_id).await;

                    if ctx.state.is_registered() {
                        // Propagate metadata to RegisteredUser
                        if let Some(user_ref) = ctx.matrix.user_manager.users.get(ctx.uid) {
                            let mut user = user_ref.write().await;
                            user.metadata = account.metadata.clone();
                        }

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
