use super::common::{
    attach_session_to_client, broadcast_account_change, encode_base64, extract_device_id,
    send_sasl_fail, send_sasl_success,
};
use crate::handlers::cap::types::SaslState;
use crate::handlers::{Context, HandlerResult};
use crate::state::client::DeviceId;
use crate::state::{SaslAccess, SessionState};
use rand::RngCore;
use ring::hmac::{self, HMAC_SHA256};
use slirc_proto::{Command, Message};
use subtle::ConstantTimeEq;
use tracing::{debug, info, warn};

/// Server nonce length in bytes.
const SCRAM_NONCE_LEN: usize = 24;

/// Generate a random server nonce component.
fn generate_server_nonce() -> String {
    let mut nonce = [0u8; SCRAM_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    encode_base64(&nonce)
}

/// Handle SCRAM client-first message.
pub(crate) async fn handle_scram_client_first<S: SessionState + SaslAccess>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    data: &str,
    _account_hint: &str,
) -> HandlerResult {
    if data == "*" {
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

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

    let username = match parse_scram_username(&client_first) {
        Some(u) => u,
        None => {
            send_sasl_fail(ctx, nick, "Invalid SCRAM client-first format").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    let (account_name, device_id) = extract_device_id(&username);

    let client_nonce = match parse_scram_nonce(&client_first) {
        Some(n) => n,
        None => {
            send_sasl_fail(ctx, nick, "Missing client nonce").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

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

    let server_nonce_component = generate_server_nonce();
    let combined_nonce = format!("{}{}", client_nonce, server_nonce_component);

    let salt_b64 = encode_base64(&verifiers.salt);
    let server_first = format!(
        "r={},s={},i={}",
        combined_nonce, salt_b64, verifiers.iterations
    );

    let client_first_bare = extract_client_first_bare(&client_first);
    let auth_message_prefix = format!("{},{}", client_first_bare, server_first);

    let server_first_b64 = encode_base64(server_first.as_bytes());
    let reply = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::AUTHENTICATE(server_first_b64),
    };
    ctx.sender.send(reply).await?;

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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_scram_client_final<S: SessionState + SaslAccess>(
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
        send_sasl_fail(ctx, nick, "SASL authentication aborted").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

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

    let client_final_nonce = match parse_scram_nonce(&client_final) {
        Some(n) => n,
        None => {
            send_sasl_fail(ctx, nick, "Invalid client-final format").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    if client_final_nonce != server_nonce {
        warn!(nick = %nick, expected = %server_nonce, got = %client_final_nonce, "SCRAM: nonce mismatch");
        send_sasl_fail(ctx, nick, "Nonce mismatch").await?;
        ctx.state.set_sasl_state(SaslState::None);
        return Ok(());
    }

    let client_proof = match parse_scram_proof(&client_final) {
        Some(p) => p,
        None => {
            send_sasl_fail(ctx, nick, "Missing client proof").await?;
            ctx.state.set_sasl_state(SaslState::None);
            return Ok(());
        }
    };

    let client_final_without_proof = build_client_final_without_proof(&client_final);
    let auth_message = format!("{},{}", auth_message_prefix, client_final_without_proof);

    let salted_password_key = hmac::Key::new(HMAC_SHA256, hashed_password);
    let client_key = hmac::sign(&salted_password_key, b"Client Key");
    let stored_key = ring::digest::digest(&ring::digest::SHA256, client_key.as_ref());
    let stored_key_hmac = hmac::Key::new(HMAC_SHA256, stored_key.as_ref());
    let client_signature = hmac::sign(&stored_key_hmac, auth_message.as_bytes());

    let mut expected_proof = [0u8; 32];
    for (i, (k, s)) in client_key
        .as_ref()
        .iter()
        .zip(client_signature.as_ref())
        .enumerate()
    {
        expected_proof[i] = k ^ s;
    }

    let authenticated = client_proof.ct_eq(&expected_proof).into();

    if authenticated {
        let server_key = hmac::sign(&salted_password_key, b"Server Key");
        let server_key_hmac = hmac::Key::new(HMAC_SHA256, server_key.as_ref());
        let server_signature = hmac::sign(&server_key_hmac, auth_message.as_bytes());

        let server_final = format!("v={}", encode_base64(server_signature.as_ref()));
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

        attach_session_to_client(ctx, account_name, device_id).await;

        if ctx.state.is_registered() {
            // Fetch account to get metadata (SCRAM verify doesn't return it)
            if let Ok(Some(account)) = ctx.db.accounts().find_by_name(account_name).await
                && let Some(user_ref) = ctx.matrix.user_manager.users.get(ctx.uid)
            {
                let mut user = user_ref.write().await;
                user.metadata = account.metadata;
            }

            broadcast_account_change(ctx, nick, account_name).await;
        }
    } else {
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

fn parse_scram_username(client_first: &str) -> Option<String> {
    let parts: Vec<&str> = client_first.splitn(3, ',').collect();
    if parts.len() < 3 {
        return None;
    }

    let bare = parts[2];
    for field in bare.split(',') {
        if let Some(username) = field.strip_prefix("n=") {
            return Some(unescape_scram_username(username));
        }
    }
    None
}

fn unescape_scram_username(s: &str) -> String {
    s.replace("=2C", ",").replace("=3D", "=")
}

fn parse_scram_nonce(msg: &str) -> Option<String> {
    for field in msg.split(',') {
        if let Some(nonce) = field.strip_prefix("r=") {
            return Some(nonce.to_string());
        }
    }
    None
}

fn extract_client_first_bare(client_first: &str) -> String {
    let parts: Vec<&str> = client_first.splitn(3, ',').collect();
    if parts.len() >= 3 {
        parts[2].to_string()
    } else {
        String::new()
    }
}

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

fn build_client_final_without_proof(client_final: &str) -> String {
    let parts: Vec<&str> = client_final.split(',').collect();
    let without_proof: Vec<&str> = parts
        .iter()
        .filter(|p| !p.starts_with("p="))
        .copied()
        .collect();
    without_proof.join(",")
}
