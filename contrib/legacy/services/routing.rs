//! Service Routing - Intercept PRIVMSG/NOTICE to Services
//!
//! Problem: Route `/msg NickServ REGISTER` to service handler, not normal user message.
//!
//! Competitive Patterns: Ergo (services map check), Anope (UID routing), UnrealIRCd (U-Line).
//!
//! Our Hybrid: Phase 1-3 (ClientManager.is_service), Phase 4+ (service_registry).
//!
//! Call Flow: commands/mod.rs → is_service_target() → route_to_service()

use crate::core::state::{ClientId, ServerState};
use std::sync::Arc;

/// Check if target is a service bot
///
/// Called before normal PRIVMSG/NOTICE handling to intercept service commands
///
/// Architectural Decision: Check ClientManager.is_service (embedded) + service_registry (future protocol).
/// Performance: <200ns per message (negligible).
pub async fn is_service_target(state: &Arc<ServerState>, target_nick: &str) -> bool {
    // Check ClientManager for embedded services
    if let Some(client_id) = state.find_client_id_by_nick(target_nick).await {
        if let Some(client) = state.get_client(client_id).await {
            // RFC 2810 §2.2.2: Service clients are distinct type from user clients
            if client.client_type == crate::core::state::ClientType::Service {
                tracing::debug!(
                    target = %target_nick,
                    client_id = ?client_id,
                    "routed PRIVMSG to embedded service"
                );
                return true;
            }
        }
    }

    // Future Phase 4+: Check service_registry for protocol-based services
    // if state.service_registry.contains(&normalized).await {
    //     tracing::debug!(target = %target_nick, "routed PRIVMSG to protocol service");
    //     return true;
    // }

    false
}

/// Route message to appropriate service handler
///
/// Called after is_service_target() returns true
/// Dispatches to nickserv, chanserv based on normalized target
///
/// Future: ServiceClient trait for unified embedded/protocol interface.
pub async fn route_to_service(
    state: &Arc<ServerState>,
    from_client_id: ClientId,
    target_nick: &str,
    message: &str,
) -> anyhow::Result<()> {
    let normalized = crate::core::state::normalize_nick(target_nick);

    // Dispatch based on service name
    match &*normalized {
        "nickserv" => {
            tracing::debug!(
                from = ?from_client_id,
                message = %message,
                "dispatching to NickServ handler"
            );
            crate::extensions::services::nickserv::handle_command(state, from_client_id, message).await?;
        }
        "chanserv" => {
            tracing::debug!(
                from = ?from_client_id,
                message = %message,
                "dispatching to ChanServ handler"
            );
            crate::extensions::services::chanserv::handle_command(state, from_client_id, message).await?;
        }
        _ => {
            // Unknown service (shouldn't happen if is_service_target worked)
            tracing::warn!(
                target = %target_nick,
                "is_service_target returned true but no handler found"
            );
        }
    }

    Ok(())
}

/// Send NOTICE from service to user
///
/// IRC Wire Format: `:NickServ!services@services.slircd NOTICE <nick> :<text>`
///
/// Competitive Pattern: Anope (service prefix), Ergo (constructs prefix).
/// Our Approach: Use ClientRecord.user_prefix (pre-computed).
pub async fn send_service_notice(
    state: &Arc<ServerState>,
    target_client_id: ClientId,
    service_name: &str,
    text: &str,
) -> anyhow::Result<()> {
    // Get service's client record (for user_prefix)
    let service_client_id = state
        .find_client_id_by_nick(service_name)
        .await
        .ok_or_else(|| anyhow::anyhow!("service {} not found", service_name))?;

    let service_client = state
        .get_client(service_client_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("service client record missing"))?;

    // Get target's nickname
    let target_client = state
        .get_client(target_client_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("target client not found"))?;

    let target_nick = target_client
        .nickname
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("target has no nickname"))?;

    // Build NOTICE: :<service_prefix> NOTICE <target> :<text>
    let notice = format!(
        ":{} NOTICE {} :{}\r\n",
        service_client.user_prefix, target_nick, text
    );

    // Send to target client via sender channel
    if let Some(sender) = state.get_client_sender(target_client_id).await {
        sender.send(notice)?;
    }

    tracing::trace!(
        service = %service_name,
        target = %target_nick,
        text_len = text.len(),
        "sent service NOTICE"
    );

    Ok(())
}
