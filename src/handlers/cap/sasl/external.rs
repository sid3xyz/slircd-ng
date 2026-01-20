use super::common::{
    attach_session_to_client, broadcast_account_change, extract_device_id, send_sasl_fail,
    send_sasl_success,
};
use crate::handlers::cap::types::SaslState;
use crate::handlers::{Context, HandlerResult};
use crate::state::{SaslAccess, SessionState};
use tracing::{info, warn};

/// Handle SASL EXTERNAL response (client confirms).
pub(crate) async fn handle_sasl_external<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
) -> HandlerResult {
    if data == "*" {
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    let device_id = if data != "+" && !data.is_empty() {
        if let Ok(decoded) = slirc_proto::sasl::decode_base64(data) {
            if let Ok(authzid) = String::from_utf8(decoded) {
                let (_, device) = extract_device_id(&authzid);
                device
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let certfp = match ctx.state.certfp() {
        Some(fp) => fp.to_string(),
        None => {
            send_sasl_fail(ctx, nick, "No client certificate provided").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    match ctx.db.accounts().find_by_certfp(&certfp).await {
        Ok(Some(account)) => {
            info!(nick = %nick, account = %account.name, "SASL EXTERNAL authentication successful");
            let account_name = account.name.clone();
            send_sasl_success(ctx, nick, &account_name).await?;
            ctx.state.set_sasl_state(SaslState::Authenticated);
            ctx.state.set_account(Some(account.name.clone()));

            attach_session_to_client(ctx, &account.name, device_id).await;

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
