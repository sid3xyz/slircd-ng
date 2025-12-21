//! STARTTLS command handler for mid-stream TLS upgrade.
//!
//! Implements RFC 7194: IRCv3 STARTTLS capability.
//!
//! STARTTLS allows clients to upgrade a plaintext connection to TLS after
//! connecting but before completing registration. The sequence is:
//!
//! 1. Client sends CAP LS, sees `tls` capability advertised
//! 2. Client sends `STARTTLS`
//! 3. Server responds with `670 :STARTTLS successful, proceed with TLS handshake`
//! 4. Server upgrades socket to TLS
//! 5. Client performs TLS handshake
//! 6. Registration continues over encrypted connection

use super::super::{Context, HandlerError, HandlerResult, PreRegHandler};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::{debug, info};

/// Handler for STARTTLS command.
///
/// `STARTTLS`
///
/// Upgrades the current plaintext connection to TLS.
/// Only valid before registration completes, and only on plaintext connections.
///
/// # IRCv3 Specification
///
/// [IRCv3 STARTTLS](https://ircv3.net/specs/deprecated/tls)
///
/// Note: While IRCv3 has deprecated STARTTLS in favor of direct TLS connections,
/// we support it for backwards compatibility with older clients.
pub struct StarttlsHandler;

#[async_trait]
impl PreRegHandler for StarttlsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let nick = ctx.state.nick.as_deref().unwrap_or("*");

        // Check if already on TLS - can't upgrade twice
        if ctx.state.is_tls {
            debug!("STARTTLS rejected: already on TLS");
            let reply = Response::err_starttls(nick, "Connection already using TLS")
                .with_prefix(ctx.server_prefix());
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Check if TLS is configured on this server
        // The handler returning StartTls will cause the lifecycle to check for TlsAcceptor
        // If none is available, it will send ERR_STARTTLS

        info!(nick = nick, "STARTTLS upgrade requested");

        // Send RPL_STARTTLS before upgrade
        let reply = Response::rpl_starttls(nick).with_prefix(ctx.server_prefix());
        ctx.sender.send(reply).await?;

        // Signal to handshake loop that TLS upgrade is needed
        // The loop will:
        // 1. Drain the response queue (sending RPL_STARTTLS)
        // 2. Perform the TLS handshake
        // 3. Update ctx.state.is_tls = true
        // 4. Continue the handshake loop
        Err(HandlerError::StartTls)
    }
}
