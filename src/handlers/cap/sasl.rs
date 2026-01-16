//! SASL authentication handler.
//!
//! Supports PLAIN, EXTERNAL, and SCRAM-SHA-256 mechanisms, both pre- and post-registration.
//! Post-registration SASL allows clients to re-authenticate to a different account.

use super::types::{SaslState, SecureString};
use crate::handlers::{Context, HandlerResult, UniversalHandler, notify_extended_monitor_watchers};
use crate::state::client::DeviceId;
use crate::state::dashmap_ext::DashMapExt;
use crate::state::{SaslAccess, SessionState};
use async_trait::async_trait;
use rand::RngCore;
use ring::hmac::{self, HMAC_SHA256};
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};
use subtle::ConstantTimeEq;
use tracing::{debug, info, warn};
use zeroize::Zeroize;

/// Extract device ID from SASL username.
///
/// SASL usernames can be in the format `account@device` where `device` is
/// used as the device identifier for bouncer/multiclient functionality.
///
/// Examples:
/// - `alice` → (account: "alice", device: None)
/// - `alice@phone` → (account: "alice", device: Some("phone"))
/// - `alice@` → (account: "alice", device: None) // empty device ignored
/// - `@phone` → (account: "", device: Some("phone")) // invalid account handled upstream
///
/// Returns the account name and optional device ID.
fn extract_device_id(username: &str) -> (String, Option<DeviceId>) {
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
///
/// This is the integration point between SASL auth and the multiclient system.
/// Called after successful authentication to register the session with the
/// account's bouncer client state.
async fn attach_session_to_client<S: SessionState + SaslAccess>(
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

    // For now, we use an empty IP string (could be enhanced to track actual IP)
    let ip = String::new();

    // Check policies for this account (per-account overrides can be added later)
    let multiclient_allowed = ctx.matrix.config.multiclient.is_multiclient_enabled(None);
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

            // If this is a reattach (not first session on this device), prepare autoreplay
            if *reattach && let Some(client_arc) = ctx.matrix.client_manager.get_client(account) {
                let client = client_arc.read().await;

                // Extract channels with membership info
                let channels: Vec<(String, crate::state::ChannelMembership)> = client
                    .channels
                    .iter()
                    .map(|(name, membership)| (name.clone(), membership.clone()))
                    .collect();

                // Get last_seen timestamp for this device (if available)
                let replay_since = device_id.as_ref().and_then(|dev| client.get_last_seen(dev));

                // Build ReattachInfo
                let reattach_info = crate::state::ReattachInfo {
                    account: account.to_string(),
                    device_id: device_id.clone(),
                    channels,
                    replay_since,
                };

                // Store in session state (will be moved to RegisteredState on registration)
                ctx.state.set_reattach_info(Some(reattach_info));

                debug!(
                    account = %account,
                    device = ?device_id,
                    channel_count = client.channels.len(),
                    has_replay_timestamp = replay_since.is_some(),
                    "Prepared autoreplay info for reattached session"
                );
            }
        }
        crate::state::managers::client::AttachResult::MulticlientNotAllowed => {
            warn!(
                account = %account,
                "Multiclient not allowed for this account"
            );
        }
        crate::state::managers::client::AttachResult::TooManySessions => {
            warn!(
                account = %account,
                "Too many sessions for account"
            );
        }
    }

    // Store device_id in session state if we have one
    if device_id.is_some() {
        ctx.state.set_device_id(device_id);
    }
}

/// Base64 encode data for SASL responses.
fn encode_base64(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

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
    } else if mechanism.eq_ignore_ascii_case("SCRAM-SHA-256") {
        // SCRAM-SHA-256: For now, we use the current nick as the account name hint.
        // The actual username will come from the client-first message.
        // We send an empty challenge; client will respond with client-first.
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

    // EXTERNAL data is usually empty (+) or authzid which may contain device ID.
    // Extract device_id from authzid if in format "account@device" or just "device"
    let device_id = if data != "+" && !data.is_empty() {
        // Decode the authzid to check for device_id
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

    // Certfp was verified in handle_sasl_init, but handle defensively
    let certfp = match ctx.state.certfp() {
        Some(fp) => fp.to_string(),
        None => {
            // Should not happen - handle_sasl_init already validated this
            send_sasl_fail(ctx, nick, "No client certificate provided").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Authenticate using CertFP
    match ctx.db.accounts().find_by_certfp(&certfp).await {
        Ok(Some(account)) => {
            info!(nick = %nick, account = %account.name, "SASL EXTERNAL authentication successful");
            let account_name = account.name.clone();
            send_sasl_success(ctx, nick, &account_name).await?;
            ctx.state.set_sasl_state(SaslState::Authenticated);
            ctx.state.set_account(Some(account.name.clone()));

            // Attach session to client manager for bouncer functionality
            attach_session_to_client(ctx, &account.name, device_id).await;

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
            // Extract device_id from authcid (e.g., "alice@phone" -> device "phone")
            let (account_from_authcid, device_id) = extract_device_id(&authcid);

            // Determine account name: prefer authzid, fall back to authcid (without device)
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

                    // Attach session to client manager for bouncer functionality
                    attach_session_to_client(ctx, &account.name, device_id).await;

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
        let Some(uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) else {
            return;
        };

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
    if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(&uid) {
        let mut user = user_arc.write().await;
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

// ============================================================================
// SCRAM-SHA-256 Implementation (Manual verification using ring)
// ============================================================================

/// Server nonce length in bytes.
const SCRAM_NONCE_LEN: usize = 24;

/// Generate a random server nonce component.
fn generate_server_nonce() -> String {
    let mut nonce = [0u8; SCRAM_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    encode_base64(&nonce)
}

/// Handle SCRAM client-first message.
///
/// Client-first format: `n,,n=<username>,r=<client-nonce>`
async fn handle_scram_client_first<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
    _account_hint: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // Decode base64 client-first message
    let client_first = match slirc_proto::sasl::decode_base64(data) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                send_sasl_fail(ctx, nick, "Invalid UTF-8 in client-first").await?;
                ctx.state.set_sasl_state(SaslState::None);
                return Ok(());
            }
        },
        Err(_) => {
            send_sasl_fail(ctx, nick, "Invalid base64").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    debug!(nick = %nick, client_first = %client_first, "SCRAM: received client-first");

    // Parse client-first to extract username and client nonce
    // Format: gs2-header,client-first-message-bare
    // gs2-header is typically "n,," (no channel binding, no authzid)
    // client-first-message-bare is "n=<username>,r=<client-nonce>[,extensions]"
    let username = match parse_scram_username(&client_first) {
        Some(u) => u,
        None => {
            send_sasl_fail(ctx, nick, "Invalid SCRAM client-first format").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Extract device_id from username (e.g., "alice@phone" -> account "alice", device "phone")
    let (account_name, device_id) = extract_device_id(&username);

    let client_nonce = match parse_scram_nonce(&client_first) {
        Some(n) => n,
        None => {
            send_sasl_fail(ctx, nick, "Missing client nonce").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Look up SCRAM verifiers for the account (without device suffix)
    let verifiers = match ctx.db.accounts().get_scram_verifiers(&account_name).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            warn!(nick = %nick, account = %account_name, device = ?device_id, "SCRAM: no verifiers for account");
            send_sasl_fail(ctx, nick, "Account not found or SCRAM not available").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
        Err(e) => {
            warn!(nick = %nick, account = %account_name, error = ?e, "SCRAM: database error");
            send_sasl_fail(ctx, nick, "Database error").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Generate combined nonce (client_nonce + server_nonce)
    let server_nonce_component = generate_server_nonce();
    let combined_nonce = format!("{}{}", client_nonce, server_nonce_component);

    // Build server-first message: r=<combined_nonce>,s=<salt>,i=<iterations>
    let salt_b64 = encode_base64(&verifiers.salt);
    let server_first = format!(
        "r={},s={},i={}",
        combined_nonce, salt_b64, verifiers.iterations
    );

    // Build auth_message_prefix = client-first-message-bare + "," + server-first-message
    // We'll complete it with client-final-message-without-proof when we receive client-final
    let client_first_bare = extract_client_first_bare(&client_first);
    let auth_message_prefix = format!("{},{}", client_first_bare, server_first);

    // Send server-first as base64
    let server_first_b64 = encode_base64(server_first.as_bytes());
    let reply = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::AUTHENTICATE(server_first_b64),
    };
    ctx.sender.send(reply).await?;

    // Store state for client-final processing
    ctx.state
        .set_sasl_state(SaslState::WaitingForScramClientFinal {
            account_name: account_name.clone(),
            device_id: device_id.clone(),
            server_nonce: combined_nonce,
            salt: verifiers.salt,
            iterations: verifiers.iterations,
            hashed_password: verifiers.hashed_password,
            auth_message: auth_message_prefix,
        });

    debug!(nick = %nick, account = %account_name, device = ?device_id, "SCRAM: sent server-first");
    Ok(())
}

/// Handle SCRAM client-final message.
///
/// Client-final format: `c=<channel-binding>,r=<nonce>,p=<proof>`
#[allow(clippy::too_many_arguments)]
async fn handle_scram_client_final<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
    account_name: &str,
    device_id: Option<DeviceId>,
    server_nonce: &str,
    _salt: &[u8],
    _iterations: u32,
    hashed_password: &[u8],
    auth_message_prefix: &str,
) -> HandlerResult {
    if data == "*" {
        // Client aborting
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // Decode base64 client-final message
    let client_final = match slirc_proto::sasl::decode_base64(data) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                send_sasl_fail(ctx, nick, "Invalid UTF-8 in client-final").await?;
                ctx.state.set_sasl_state(SaslState::None);
                return Ok(());
            }
        },
        Err(_) => {
            send_sasl_fail(ctx, nick, "Invalid base64").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    debug!(nick = %nick, client_final = %client_final, "SCRAM: received client-final");

    // Parse client-final: c=<channel-binding>,r=<nonce>,p=<proof>
    let client_final_nonce = match parse_scram_nonce(&client_final) {
        Some(n) => n,
        None => {
            send_sasl_fail(ctx, nick, "Invalid client-final format").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Verify the nonce matches what we sent
    if client_final_nonce != server_nonce {
        warn!(nick = %nick, expected = %server_nonce, got = %client_final_nonce, "SCRAM: nonce mismatch");
        send_sasl_fail(ctx, nick, "Nonce mismatch").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    // Parse client proof
    let client_proof = match parse_scram_proof(&client_final) {
        Some(p) => p,
        None => {
            send_sasl_fail(ctx, nick, "Missing client proof").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    // Build client-final-message-without-proof for auth_message
    let client_final_without_proof = build_client_final_without_proof(&client_final);

    // Complete auth_message: client-first-bare + "," + server-first + "," + client-final-without-proof
    let auth_message = format!("{},{}", auth_message_prefix, client_final_without_proof);

    // Verify the client proof using RFC 5802 formulas:
    // SaltedPassword := Hi(password, salt, i)  -- already stored as hashed_password
    // ClientKey := HMAC(SaltedPassword, "Client Key")
    // StoredKey := H(ClientKey)
    // ClientSignature := HMAC(StoredKey, AuthMessage)
    // ClientProof := ClientKey XOR ClientSignature
    //
    // To verify, we compute: ClientKey' = ClientProof XOR ClientSignature
    // Then verify: H(ClientKey') == StoredKey (which we'd need to store separately)
    //
    // Actually, we store the SaltedPassword (hashed_password), so:
    // 1. Compute ClientKey from SaltedPassword
    // 2. Compute StoredKey = H(ClientKey)
    // 3. Compute ClientSignature = HMAC(StoredKey, auth_message)
    // 4. Compute expected_proof = ClientKey XOR ClientSignature
    // 5. Compare with received proof

    let salted_password_key = hmac::Key::new(HMAC_SHA256, hashed_password);
    let client_key = hmac::sign(&salted_password_key, b"Client Key");
    let stored_key = ring::digest::digest(&ring::digest::SHA256, client_key.as_ref());
    let stored_key_hmac = hmac::Key::new(HMAC_SHA256, stored_key.as_ref());
    let client_signature = hmac::sign(&stored_key_hmac, auth_message.as_bytes());

    // Compute expected client proof: ClientKey XOR ClientSignature
    let mut expected_proof = [0u8; 32];
    for (i, (k, s)) in client_key
        .as_ref()
        .iter()
        .zip(client_signature.as_ref())
        .enumerate()
    {
        expected_proof[i] = k ^ s;
    }

    // Verify proof matches using constant-time comparison to prevent timing attacks
    let authenticated = client_proof.ct_eq(&expected_proof).into();

    if authenticated {
        // Compute server signature for server-final
        // ServerKey := HMAC(SaltedPassword, "Server Key")
        // ServerSignature := HMAC(ServerKey, AuthMessage)
        let server_key = hmac::sign(&salted_password_key, b"Server Key");
        let server_key_hmac = hmac::Key::new(HMAC_SHA256, server_key.as_ref());
        let server_signature = hmac::sign(&server_key_hmac, auth_message.as_bytes());

        // Build server-final: v=<server-signature>
        let server_final = format!("v={}", encode_base64(server_signature.as_ref()));

        // Send server-final
        let server_final_b64 = encode_base64(server_final.as_bytes());
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE(server_final_b64),
        };
        ctx.sender.send(reply).await?;

        info!(nick = %nick, account = %account_name, device = ?device_id, "SASL SCRAM-SHA-256 authentication successful");
        send_sasl_success(ctx, nick, account_name).await?;
        ctx.state.set_sasl_state(SaslState::Authenticated);
        ctx.state.set_account(Some(account_name.to_string()));

        // Attach session to client manager for bouncer functionality
        attach_session_to_client(ctx, account_name, device_id).await;

        // Broadcast account change if post-registration
        if ctx.state.is_registered() {
            broadcast_account_change(ctx, nick, account_name).await;
        }
    } else {
        // Authentication failed - send server-final with error
        let server_final = "e=invalid-proof";
        let server_final_b64 = encode_base64(server_final.as_bytes());
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::AUTHENTICATE(server_final_b64),
        };
        ctx.sender.send(reply).await?;

        warn!(nick = %nick, account = %account_name, "SASL SCRAM-SHA-256 authentication failed");
        send_sasl_fail(ctx, nick, "Authentication failed").await?;
        ctx.state.set_sasl_state(SaslState::None);
    }

    Ok(())
}

/// Parse username from SCRAM client-first message.
///
/// Format: `gs2-header,client-first-message-bare`
/// Where client-first-message-bare contains `n=<username>,r=<nonce>`
fn parse_scram_username(client_first: &str) -> Option<String> {
    // Skip gs2-header (typically "n,," or "y,,")
    let parts: Vec<&str> = client_first.splitn(3, ',').collect();
    if parts.len() < 3 {
        return None;
    }

    // parts[2] is the client-first-message-bare
    let bare = parts[2];

    // Find n=<username>
    for field in bare.split(',') {
        if let Some(username) = field.strip_prefix("n=") {
            // SCRAM usernames can have escape sequences
            return Some(unescape_scram_username(username));
        }
    }
    None
}

/// Unescape SCRAM username (=2C -> , and =3D -> =).
fn unescape_scram_username(s: &str) -> String {
    s.replace("=2C", ",").replace("=3D", "=")
}

/// Parse nonce (r=) from SCRAM message.
fn parse_scram_nonce(msg: &str) -> Option<String> {
    for field in msg.split(',') {
        if let Some(nonce) = field.strip_prefix("r=") {
            return Some(nonce.to_string());
        }
    }
    None
}

/// Extract client-first-message-bare from full client-first message.
fn extract_client_first_bare(client_first: &str) -> String {
    // Skip gs2-header
    let parts: Vec<&str> = client_first.splitn(3, ',').collect();
    if parts.len() >= 3 {
        parts[2].to_string()
    } else {
        String::new()
    }
}

/// Parse client proof (p=) from SCRAM client-final message.
/// Returns decoded proof bytes.
fn parse_scram_proof(client_final: &str) -> Option<[u8; 32]> {
    for field in client_final.split(',') {
        if let Some(proof_b64) = field.strip_prefix("p=")
            && let Ok(proof_bytes) = slirc_proto::sasl::decode_base64(proof_b64)
            && proof_bytes.len() == 32
        {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&proof_bytes);
            return Some(arr);
        }
    }
    None
}

/// Build client-final-message-without-proof from full client-final.
/// Client-final format: c=<channel-binding>,r=<nonce>,p=<proof>
/// We need: c=<channel-binding>,r=<nonce>
fn build_client_final_without_proof(client_final: &str) -> String {
    let parts: Vec<&str> = client_final.split(',').collect();
    // Find all parts except p=...
    let without_proof: Vec<&str> = parts
        .iter()
        .filter(|p| !p.starts_with("p="))
        .copied()
        .collect();
    without_proof.join(",")
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
