//! SASL authentication handler.
//!
//! Supports PLAIN, EXTERNAL, and SCRAM-SHA-256 mechanisms, both pre- and post-registration.
//! Post-registration SASL allows clients to re-authenticate to a different account.

mod common;
mod external;
mod plain;
mod scram;

use super::types::SaslState;
use crate::handlers::{Context, HandlerResult, UniversalHandler};
use crate::state::{SaslAccess, SessionState};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef};
use tracing::debug;

use common::send_sasl_fail;
use external::handle_sasl_external;
use plain::handle_sasl_plain_data;
use scram::{handle_scram_client_first, handle_scram_client_final};

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

        match ctx.state.sasl_state().clone() {
            SaslState::None => handle_sasl_init(ctx, &nick, data).await,
            SaslState::WaitingForExternal => handle_sasl_external(ctx, &nick, data).await,
            SaslState::WaitingForData => handle_sasl_plain_data(ctx, &nick, data).await,
            SaslState::WaitingForScramClientFirst { account_name } => {
                handle_scram_client_first(ctx, &nick, data, &account_name).await
            }
            SaslState::WaitingForScramClientFinal {
                account_name,
                device_id,
                server_nonce,
                salt,
                iterations,
                hashed_password,
                auth_message,
            } => {
                handle_scram_client_final(
                    ctx,
                    &nick,
                    data,
                    &account_name,
                    device_id,
                    &server_nonce,
                    &salt,
                    iterations,
                    &hashed_password,
                    &auth_message,
                )
                .await
            }
            SaslState::Authenticated => {
                // Already authenticated - allow re-authentication by starting fresh
                debug!(nick = %nick, "AUTHENTICATE after authenticated, starting fresh");
                ctx.state.set_sasl_state(SaslState::None);
                handle_sasl_init(ctx, &nick, data).await
            }
        }
    }
}

/// Handle SASL initiation - client sends mechanism name (PLAIN, EXTERNAL, or SCRAM-SHA-256).
async fn handle_sasl_init<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    mechanism: &str,
) -> HandlerResult {
    if mechanism.eq_ignore_ascii_case("PLAIN") {
        if !ctx.state.is_tls() && !ctx.matrix.config.security.allow_plaintext_sasl_plain {
            send_sasl_fail(ctx, nick, "SASL PLAIN requires TLS connection").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }

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

        if ctx.state.certfp().is_none() {
             send_sasl_fail(ctx, nick, "No client certificate provided").await?;
             ctx.state.set_sasl_state(SaslState::None);
             return Ok(());
        }

        ctx.state.set_sasl_state(SaslState::WaitingForExternal);
        // Send empty challenge
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE("+".to_string()),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, "SASL EXTERNAL: sent challenge");
    } else if mechanism.eq_ignore_ascii_case("SCRAM-SHA-256") {
        // SCRAM-SHA-256: For now, we use the current nick as the account name hint.
        ctx.state
            .set_sasl_state(SaslState::WaitingForScramClientFirst {
                account_name: nick.to_string(),
            });
        // Send empty challenge (AUTHENTICATE +)
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE("+".to_string()),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, "SASL SCRAM-SHA-256: sent initial challenge");
    } else {
        // Unsupported mechanism
        send_sasl_fail(ctx, nick, "Unsupported mechanism").await?;
        ctx.state.set_sasl_state(SaslState::None);
    }
    Ok(())
}
