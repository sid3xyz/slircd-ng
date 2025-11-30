//! Pseudo-Client System - Embedded Services as Fake IRC Clients
//!
//! Architecture: Services as regular clients (Ergo pattern) for natural network behavior.
//! Our Enhancement: Store in ServerState.clients (embedded) + service_registry (future protocol).
//! Lifecycle: Spawn on server start, graceful QUIT on shutdown.

use crate::infrastructure::config::Config;
use crate::core::state::{ClientId, ClientRecord, ServerState, UserModes};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::Notify;

/// Service bot handles - stored for graceful shutdown
#[derive(Debug, Clone)]
pub struct ServiceBots {
    pub nickserv_id: Option<ClientId>,
    pub chanserv_id: Option<ClientId>,
}

/// Spawn IRC services pseudo-clients
///
/// Called on server start if config.services.enabled = true
/// Creates NickServ and ChanServ as fake clients in ServerState.clients
///
/// Ergo Pattern: Services spawned in initializeServices()
/// Our Enhancement: Store in ClientManager for unified client handling
pub async fn spawn_services(state: Arc<ServerState>, config: Arc<Config>) -> Result<ServiceBots> {
    if !config.services.enabled {
        tracing::info!("services disabled in config, skipping spawn");
        return Ok(ServiceBots {
            nickserv_id: None,
            chanserv_id: None,
        });
    }

    tracing::info!(
        nickserv = %config.services.nickserv_name,
        chanserv = %config.services.chanserv_name,
        "spawning IRC services pseudo-clients (embedded model)"
    );

    // Spawn NickServ
    let nickserv_id = create_service_client(
        Arc::clone(&state),
        config.services.nickserv_name.clone(),
        "services".to_string(),
        "Nickname Services".to_string(),
    )
    .await
    .context("failed to spawn NickServ")?;

    tracing::info!(client_id = ?nickserv_id, "NickServ spawned successfully");

    // Spawn ChanServ
    let chanserv_id = create_service_client(
        Arc::clone(&state),
        config.services.chanserv_name.clone(),
        "services".to_string(),
        "Channel Services".to_string(),
    )
    .await
    .context("failed to spawn ChanServ")?;

    tracing::info!(client_id = ?chanserv_id, "ChanServ spawned successfully");

    Ok(ServiceBots {
        nickserv_id: Some(nickserv_id),
        chanserv_id: Some(chanserv_id),
    })
}

/// Create a single service pseudo-client
///
/// Competitive Pattern: Anope (UID +S mode), Ergo (separate services map).
/// Our Hybrid: in ClientManager (embedded) + service_registry (future).
///
/// ClientRecord: nickname, username="services", realname, dummy addr/socket, welcomed=true, is_service=true (bypass limits), account=None.
async fn create_service_client(
    state: Arc<ServerState>,
    nickname: String,
    username: String,
    realname: String,
) -> Result<ClientId> {
    let client_id = state.next_client_id();

    // Dummy socket address (services have no real connection)
    let dummy_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

    // Dummy outbound channel (services send via state.send_to_client, not this channel)
    let (tx, _rx) = mpsc::unbounded_channel();

    // Hostname for services (appears in WHOIS, prefixes)
    let service_hostname = "services.slircd".to_string();

    // Pre-compute user prefix (cached for performance)
    let user_prefix = format!("{}!{}@{}", nickname, username, service_hostname);

    let record = ClientRecord {
        client_type: crate::core::state::ClientType::Service, // RFC 2810 §2.2.2: Services are service type
        nickname: Some(nickname.clone()),
        username: Some(username.clone()),
        realname: Some(realname),
        away_message: None,
        real_username: Some(username.clone()), // PHASE 6C: Services have static usernames
        display_username: Some(username),      // PHASE 6C: No cloaking for services
        sender: tx,
        peer_addr: dummy_addr,
        welcomed: true, // Services are always "registered"
        user_modes: UserModes::default(),
        active_caps: HashSet::new(),
        account: None, // Services don't authenticate (they ARE the authentication system)
        cap_version: 302,
        cap_negotiation_active: false, // Services: Bypass CAP negotiation (already registered)
        monitor_list: HashSet::new(),
        sasl_mechanism: None,
        certificate_fp: None,
        password_validated: true,
        permission_level: 2000, // Admin-level (services have full privileges)
        server_id: 0,           // Local service (not remote S2S)
        real_ip: None,
        real_hostname: None,
        gateway_name: None,
        cloaked_hostname: None,
        display_hostname: service_hostname,
        user_prefix,
        last_activity: Instant::now(),
        last_ping_sent: None,
        disconnect_notify: Arc::new(Notify::new()),
        // PHASE 5D.3: Services get timestamps (for consistency)
        created_at: chrono::Utc::now().timestamp(),
        nick_ts: chrono::Utc::now().timestamp(),
        // STRAYLIGHT Phase 4.2: remote_client_id field deleted
        // STRAYLIGHT: Services get UID for S2S protocol compatibility
        uid: None, // Could generate UID for services, but they don't route via S2S
    };

    // Insert into ClientManager (unified client handling)
    state.insert_service_client(client_id, record.clone()).await;

    tracing::debug!(
        client_id = ?client_id,
        nickname = %nickname,
        "service pseudo-client created in ClientManager"
    );

    Ok(client_id)
}
